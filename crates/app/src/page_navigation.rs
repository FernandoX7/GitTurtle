//! Page transitions share one retained repository. Inspections keep their
//! existing bounded context stacks; page visits never rebuild that workspace.
use crate::*;

pub(super) fn return_page(page: AppPage, origin: AppPage, repository: bool) -> AppPage {
    match page {
        AppPage::Settings if origin == AppPage::Projects => AppPage::Projects,
        _ if repository => AppPage::Repository,
        _ => AppPage::Projects,
    }
}

/// The window's wait for a review's preparation a switch dropped while its
/// read may still run on the operation executor
/// ([`GitTurtle::wait_for_dropped_preparation`]). The window holds at most
/// one, and ending it drops its task, so its end runs only while the window
/// still holds it.
pub(super) struct DroppedPreparation {
    /// The dropped preparation's label, which the busy state shows until
    /// the wait ends in the tab that asked for it; any other tab shows
    /// where the work runs instead ([`FINISHING_ELSEWHERE`]).
    label: &'static str,
    /// The repository whose tab asked for the preparation, while that tab
    /// is open ([`GitTurtle::forget_dropped_preparation_tab`]), which decides
    /// only the label the wait shows ([`GitTurtle::dropped_preparation_label`]):
    /// opened again in place, as History's Refresh opens it, every tab holds
    /// the wait alike ([`GitTurtle::hold_dropped_preparation`]).
    repository: Option<PathBuf>,
    /// The wait for the marker queued behind the preparation's read.
    _wait: Task<()>,
}

impl DroppedPreparation {
    /// Whether the busy state `busy` is this wait's, under its own label or
    /// one it shows in another tab.
    fn holds(&self, busy: &str) -> bool {
        busy == self.label || finishing_elsewhere_reason(busy).is_some()
    }
}

/// What a wait for a dropped preparation shows as its busy label in a tab
/// other than the one that asked for it, with the reason a control it
/// refuses gives ([`GitTurtle::busy_reason`]): while that tab is open, and
/// once it has closed. That tab's own label ("Reviewing file changes…")
/// once stood as the reason the shown tab's actions were unavailable, for
/// work the shown tab never asked for. Fixed strings, as every busy label
/// is, they name the tab's kind rather than its project, and run no longer
/// than a GitHub request's label; like any busy label they go only where
/// they move nothing, so nothing moves as they show.
const FINISHING_ELSEWHERE: [(&str, &str); 2] = [
    (
        "Finishing another tab's review…",
        "Wait for another tab's review to finish",
    ),
    (
        "Finishing a closed tab's review…",
        "Wait for a closed tab's review to finish",
    ),
];

/// Why a control waits while the busy state shows `label`, where that is
/// a wait's label in another tab ([`FINISHING_ELSEWHERE`]).
pub(crate) fn finishing_elsewhere_reason(label: &str) -> Option<&'static str> {
    FINISHING_ELSEWHERE
        .iter()
        .find(|(shown, _)| *shown == label)
        .map(|(_, reason)| *reason)
}

/// Where a page switch goes, which decides what becomes of the reviews
/// being prepared as the window leaves the view that asked for them
/// ([`GitTurtle::end_preparations`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum PageSwitch {
    /// To Settings or Projects, off the repository page, or to Projects as
    /// the last repository tab closes.
    Away,
    /// To Changes, over History or Compare (`show_working`).
    ToChanges,
    /// To History, from Changes (`back_to_history`, which `show_history`
    /// runs).
    ToHistory,
    /// To another repository tab, a new tab's or one already open, from
    /// the tab shown (`retain_active_tab`), or as that tab closes
    /// (`close_repository_tab`).
    Tab,
}

impl GitTurtle {
    /// Ends the reviews being prepared as a page switch leaves the view
    /// that asked for them, so none opens its form or confirmation over the
    /// view shown next, which once took focus from where the person had
    /// put it: the one set every switch applies, Settings and Projects
    /// (`show_settings`, `show_projects`), Changes opened over History or
    /// Compare (`show_working`), History shown again from Changes
    /// (`back_to_history`) and another repository tab shown. Each
    /// preparation is cancelled, dismissed or dropped:
    ///
    /// | Preparation | Settings, Projects | History to Changes | Changes to History | Another tab |
    /// | --- | --- | --- | --- | --- |
    /// | Merge…/Rebase… branch comparison | cancelled | dropped | dropped | dropped |
    /// | Tags, a tag's details, Create tag's review | cancelled | dropped | dropped | dropped |
    /// | Ignore…'s review, Preview rule | cancelled | dropped | dropped | dropped |
    /// | Discard…, Delete untracked file…'s review | cancelled | dropped | dropped | dropped |
    /// | Recovery plans: Amend…, Revert…, Review restore… | cancelled | dismissed | History's wanted again, Changes' dropped | dropped |
    /// | Edit local commits, a paused rebase's Continue | cancelled | dismissed | History's wanted again, Changes' dropped | dropped |
    /// | Branch actions | cancelled | dismissed | History's wanted again, Changes' dropped | dropped |
    ///
    /// A cancel drops the preparation and ends its busy state at once. Its
    /// end, which reads the working state asked for meanwhile, never runs,
    /// so this reads it ([`Self::read_noted_working`]): off the repository
    /// page nothing else reads, and Projects never waits for a review.
    ///
    /// A dismissal drops only the plan: the read, which the executor cannot
    /// stop, ends as it would, keeping the busy state until then, and its
    /// end reads; History shown again wants a plan it asked for back, which
    /// then opens there ([`Self::dismiss_history_plans`]). Only those three
    /// carry a dismissal of their own, and each records whether Changes
    /// asked for it as it starts (`recovery_asked_in_changes`): History
    /// shown again drops one Changes asked for, as it drops Changes'
    /// Discard…, rather than want it, which once opened Amend last commit's
    /// form, a branch picker action's or Continue's over History. The rest
    /// are dropped: cancelled, so
    /// nothing they bring opens, while the window keeps their busy state
    /// until the executor reaches a marker queued behind their read, whose
    /// end reads ([`Self::wait_for_dropped_preparation`]). Cancelled
    /// outright, each switch to Changes during a slow read queued a status
    /// read behind it, until the executor's bounded queue refused one and
    /// Changes lost its rows.
    ///
    /// Another tab drops every kind, for that same reason: the read still
    /// runs on the one executor every tab shares, so the next tab's
    /// Changes would queue its reads behind it as History's switch to
    /// Changes did. The tab that asked is no longer shown, so no plan is
    /// kept for it to want again: shown again, it opens nothing it asked
    /// for before it was left, and its working read, which the switch drops
    /// with the tab's other reads, is read quietly instead. A tab switch
    /// once ended only Discard…'s review and Merge…'s comparison, at once,
    /// and let the other kinds carry on, so their forms and dialogs opened
    /// over the tab that asked once it was shown again, and a "Comparing
    /// branch tips…" a wait held went on into the next tab while a held
    /// "Reviewing file changes…" ended.
    ///
    /// Settings once left the recovery, Tags and Ignore preparations, and
    /// Merge…'s comparison, to open over it, Projects waited for them as
    /// for a write, Changes let Tags and Merge…'s confirmation open over
    /// the working list, and History, shown from Changes, let Discard…'s
    /// open over the history.
    pub(super) fn end_preparations(
        &mut self,
        switch: PageSwitch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let busy = self.operation_busy;
        self.cancel_integration_comparison();
        self.cancel_tag_action();
        self.cancel_ignore_action();
        self.cancel_discard_action();
        match switch {
            PageSwitch::Away => {
                self.cancel_recovery_read();
                self.cancel_interactive_rebase_action();
                self.cancel_branch_action();
                self.end_dropped_preparation();
                self.read_noted_working(window, cx);
                return;
            }
            PageSwitch::ToChanges => self.dismiss_history_plans(true),
            PageSwitch::ToHistory => {
                // A plan Changes asked for is dropped, as its Discard…'s
                // review is; one History asked for is wanted again.
                if self.recovery_asked_in_changes() {
                    self.cancel_recovery_read();
                }
                if self.interactive_rebase_asked_in_changes() {
                    self.cancel_interactive_rebase_action();
                }
                if self.branch_action_asked_in_changes() {
                    self.cancel_branch_action();
                }
                self.dismiss_history_plans(false);
            }
            PageSwitch::Tab => {
                self.cancel_recovery_read();
                self.cancel_interactive_rebase_action();
                self.cancel_branch_action();
            }
        }
        self.wait_for_cancelled_preparation(busy, window, cx);
    }

    /// Waits for the preparation a cancel just ended, whose read may still
    /// run on the operation executor ([`Self::wait_for_dropped_preparation`]):
    /// `busy` is the busy state before the cancels ran, and one that ended it
    /// cancelled the preparation it was, or took a running wait's label,
    /// which the wait then takes back. Every switch that drops
    /// (`end_preparations`) and `open` in place, as History's Refresh opens
    /// the repository shown, run their cancels through it. Refresh once
    /// cancelled a recovery plan, a branch's actions or Edit local commits
    /// History was preparing without a wait, so each press during a slow
    /// read queued a status read behind it, each keeping its slot of the
    /// executor's bounded queue though the next press dropped it, until one
    /// was refused as busy and Changes lost its rows.
    pub(super) fn wait_for_cancelled_preparation(
        &mut self,
        busy: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(label) = busy
            && self.operation_busy.is_none()
        {
            self.wait_for_dropped_preparation(label, window, cx);
        }
    }

    /// Keeps the window busy as `label`, the preparation a switch or `open`
    /// in place just dropped ([`Self::wait_for_cancelled_preparation`]),
    /// until the operation executor
    /// reaches a marker queued behind that preparation's read, which may
    /// still run, and then ends the busy state and reads what was asked for
    /// meanwhile ([`Self::read_deferred_working`]), as a dismissed
    /// preparation's end does.
    ///
    /// The window holds the wait itself, not its label: nothing starts
    /// while it holds the busy state, and whatever ends that state
    /// meanwhile ends the wait with it, dropping its task so its end never
    /// runs (Settings, Projects and the last tab's close,
    /// [`Self::end_dropped_preparation`]), or takes it back for the wait
    /// ([`Self::hold_dropped_preparation`]). So a wait already running
    /// holds the very preparation dropped again, as each switch back and
    /// forth drops it, and goes on rather than queue another marker, and its
    /// end ends only its own busy state. Keyed on the label, a wait that a
    /// tab switch had stripped of its busy state went on, so Discard…'s
    /// review started again in the next tab, under the same label, queued
    /// behind its marker, and the marker's end ended that review's busy
    /// state while it still ran: a Commit was accepted meanwhile, and the
    /// review's confirmation opened as it ran.
    fn wait_for_dropped_preparation(
        &mut self,
        label: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hold_dropped_preparation() {
            return;
        }
        self.operation_busy = Some(label);
        let reached = self.operations.submit_read(|| Ok(()));
        let wait = cx.spawn_in(window, async move |this, cx| {
            let _ = reached.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.end_dropped_preparation();
                this.read_deferred_working(window, cx);
                cx.notify();
            });
        });
        self.dropped_preparation = Some(DroppedPreparation {
            label,
            repository: self.path.clone(),
            _wait: wait,
        });
    }

    /// Gives a running wait for a dropped preparation
    /// ([`Self::wait_for_dropped_preparation`]) back the busy state it
    /// holds, which a cancel run meanwhile ended by its label, and says
    /// whether one runs. Every switch that drops (`end_preparations`), a
    /// tab switch and a tab close among them, and `open` in place, as
    /// History's Refresh opens the repository shown, in whichever tab, cancel
    /// each kind first, by labels a wait may hold: the read it waits for
    /// still runs on the executor every tab shares, so the wait goes on, into
    /// the tab shown next too, and that tab's reads, a Refresh's snapshot's
    /// among them, are noted for its end rather than queued behind the read.
    /// Ended by Refresh in place, in the tab that asked or in another, the
    /// wait let each snapshot queue a status read behind that read, each
    /// keeping its slot of the executor's bounded queue until it reached the
    /// front though the next Refresh had dropped it, so a few presses filled
    /// the queue, the next read was refused as busy and Changes lost its
    /// rows. The only ways out of a read that never ends are those that end
    /// the wait ([`Self::end_dropped_preparation`]): Settings, from wherever
    /// it is offered, Projects, from its shortcut, the header's Projects,
    /// History's Back to Projects, File ▸ Projects…, the palette or the
    /// Linux Main Menu, none of which the wait refuses
    /// (`GitTurtle::projects_refusal`), and closing the last tab, which
    /// shows Projects.
    ///
    /// The busy state it gives back is the one for the repository shown
    /// ([`Self::dropped_preparation_label`]), so each change of the shown
    /// repository calls it too: a tab switched to, warm or cold, or opened
    /// anew (`restore_warm_tab`, `open`), and a tab closed. Nothing else
    /// holds the busy state while the wait runs, since nothing starts.
    pub(super) fn hold_dropped_preparation(&mut self) -> bool {
        let Some(label) = self.dropped_preparation_label() else {
            return false;
        };
        self.operation_busy = Some(label);
        true
    }

    /// Whether the busy state is only a running wait for a dropped
    /// preparation, under whichever label it shows
    /// ([`DroppedPreparation::holds`]): no write runs, nor any preparation
    /// still to answer.
    fn waits_only_for_dropped_preparation(&self) -> bool {
        self.operation_busy.is_some_and(|busy| {
            self.dropped_preparation
                .as_ref()
                .is_some_and(|running| running.holds(busy))
        })
    }

    /// Whether Projects ends the busy state as it opens, so it stays offered
    /// wherever it is (`GitTurtle::projects_refusal`): the busy state is a
    /// review being prepared, which Settings and Projects cancel
    /// ([`Self::end_preparations`], under the labels its cancels end), or a
    /// running wait for a dropped one, which they end, and no write.
    /// Refused while a review was prepared, the header's Projects and
    /// History's Back to Projects looked and were announced unavailable,
    /// giving the review's label as their reason, while Projects' shortcut
    /// opened it and cancelled the review.
    pub(crate) fn projects_ends_busy(&self) -> bool {
        self.waits_only_for_dropped_preparation()
            || self.operation_busy.is_some_and(|busy| {
                [
                    crate::discard::PREPARING,
                    crate::tags::PREPARING,
                    crate::ignore::PREPARING,
                    crate::recovery::PREPARING,
                    crate::interactive_rebase::PREPARING,
                    crate::branch_actions::PREPARING,
                ]
                .contains(&busy)
                    || (busy == crate::integration::COMPARING && self.integration_task.is_some())
            })
    }

    /// The busy label of a running wait for a dropped preparation in the
    /// repository shown: the preparation's own in the tab that asked for
    /// it, and in any other tab what it finishes and where
    /// ([`FINISHING_ELSEWHERE`]): another tab, or a closed one once that
    /// tab has closed ([`Self::forget_dropped_preparation_tab`]).
    fn dropped_preparation_label(&self) -> Option<&'static str> {
        let running = self.dropped_preparation.as_ref()?;
        Some(match &running.repository {
            Some(asked) if self.path.as_ref() == Some(asked) => running.label,
            Some(_) => FINISHING_ELSEWHERE[0].0,
            None => FINISHING_ELSEWHERE[1].0,
        })
    }

    /// Forgets which tab asked for a running wait's preparation as that
    /// tab, whose repository is `closed`, closes, so every tab then says the
    /// wait finishes a closed tab's review ([`FINISHING_ELSEWHERE`]), a tab
    /// that opens the same repository again among them, since that tab never
    /// asked. Found by its repository's path while the record outlived the
    /// tab, such a tab once showed the review's own label, "Reviewing file
    /// changes…", with the Git operation's reason, for work it never asked
    /// for.
    pub(super) fn forget_dropped_preparation_tab(&mut self, closed: &std::path::Path) {
        if let Some(running) = self.dropped_preparation.as_mut()
            && running.repository.as_deref() == Some(closed)
        {
            running.repository = None;
        }
    }

    /// Ends the wait for a dropped preparation
    /// ([`Self::wait_for_dropped_preparation`]) with its busy state, under
    /// whichever label it shows, as a cancel ends a preparation's:
    /// Settings, Projects and the last tab's close cancel what it waits for,
    /// in whichever tab, since [`DroppedPreparation::holds`] knows each of
    /// its labels. Its task goes with it, so the wait's end never runs
    /// afterwards: a wait starts only while none runs, and nothing else
    /// replaces it, so a generation the end once compared with the window's
    /// never differed.
    fn end_dropped_preparation(&mut self) {
        if let Some(running) = self.dropped_preparation.take()
            && self.operation_busy.is_some_and(|busy| running.holds(busy))
        {
            self.operation_busy = None;
        }
    }

    pub(super) fn update_modal_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let modal_open = window.has_active_dialog(cx) || window.has_active_sheet(cx);
        if self.modal_was_open != modal_open {
            self.modal_focus_generation = self.modal_focus_generation.wrapping_add(1);
        }
        if self.modal_was_open && !modal_open {
            let previous = self.modal_return_focus.take();
            let replaced = self.modal_replaced_focus.take();
            let generation = self.modal_focus_generation;
            // Where the close left focus: the toolkit gives it back to the
            // control it captured as the modal opened, as the modal closes
            // (`vendor/gpui-component`, `Dialog`'s close request). The repair
            // is the one return after that, and only while focus stays
            // there: a click or a key the person gives first wins. A recheck
            // 300 ms later once followed the toolkit's own return, which
            // waited out a 250 ms close animation, and each took focus from a
            // control clicked meanwhile.
            let left = window.focused(cx).map(|focus| focus.downgrade());
            let owner = cx.entity().downgrade();
            window.on_next_frame(move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
                        this.keep_closing_modal_return(previous, replaced);
                        return;
                    }
                    if window.focused(cx).map(|focus| focus.downgrade()) != left {
                        return;
                    }
                    this.restore_detached_modal_focus(
                        generation,
                        previous.as_ref(),
                        replaced.as_ref(),
                        window,
                        cx,
                    );
                });
            });
        } else if !modal_open {
            // A modal opened and closed with no frame between, a restore
            // run under it, leaves its replaced control behind, which a
            // later modal would keep as its own (`return_modal_to`).
            self.modal_replaced_focus = None;
            if self.app_focus.contains_focused(window, cx) {
                self.modal_return_focus = window.focused(cx);
            }
        }
        self.modal_was_open = modal_open;
    }

    /// Hands a closing modal's return to a modal opened before its focus
    /// came back, in the frame after the close: focus then rests where the
    /// close left it, the control the toolkit captured as the first modal
    /// opened, which the new one captures as its opener, while the control
    /// the first returns to, `previous`, can be another one, as the GitHub
    /// panel's recorded opener or a restore's control under the modal
    /// (`return_modal_to`) are. So focus returns there as the last modal
    /// closes, and a modal opened over another keeps the first's return
    /// as before. The return was taken as the first closed, so the new
    /// modal returned to whatever focus the close had left, the replaced
    /// page's control among them.
    fn keep_closing_modal_return(
        &mut self,
        previous: Option<FocusHandle>,
        replaced: Option<FocusHandle>,
    ) {
        if previous.is_some() {
            self.modal_return_focus = previous;
        }
        if self.modal_replaced_focus.is_none() {
            self.modal_replaced_focus = replaced;
        }
    }

    /// Repairs focus a closing modal left on nothing drawn, or on the
    /// window's root, which the toolkit gives back to a modal opened after
    /// a click on an empty spot of the window and which shows no focus, or
    /// on `replaced`, the control the toolkit captured as the modal opened
    /// where the page behind it was restored meanwhile
    /// ([`Self::return_modal_to`]): that control belongs to the page the
    /// restore replaced, drawn or not.
    fn restore_detached_modal_focus(
        &mut self,
        generation: u64,
        previous: Option<&FocusHandle>,
        replaced: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal_focus_generation != generation
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        let on_replaced = replaced.is_some_and(|replaced| replaced.is_focused(window));
        if !on_replaced && !window.context_stack().is_empty() && !self.app_focus.is_focused(window)
        {
            return;
        }
        self.modal_return_target(previous, window).focus(window, cx);
        window.refresh();
    }

    /// Makes `focus` the control the open dialog or sheet gives focus back
    /// to as it closes, for a restore of the page behind it, which moves no
    /// focus out of the modal (`restore_inspection_context`): focus on a
    /// control behind a modal is drawn nowhere a person can see and sends
    /// the next key there. The control the toolkit captured as the modal
    /// opened belonged to the page the restore replaced; one still drawn,
    /// as File history's list is Compare's file list too, would otherwise
    /// keep focus as the modal closed, so the repair gives way from it
    /// (`restore_detached_modal_focus`). `None`, as for the window's root,
    /// returns to the shown mode's own list (`modal_return_target`).
    pub(super) fn return_modal_to(&mut self, focus: Option<FocusHandle>) {
        let replaced = std::mem::replace(&mut self.modal_return_focus, focus);
        // A second restore under the same modal, as `close_inspections`
        // closes one inspection after another, keeps the first's: only that
        // control is the one the toolkit captured.
        if self.modal_replaced_focus.is_none() {
            self.modal_replaced_focus = replaced;
        }
    }

    /// `focus` as a place to give focus back to: none where it is the
    /// window's root, which shows no focus and acts on nothing. A press on
    /// an empty spot of the window focuses the root, since GPUI focuses an
    /// element that tracks focus on any press inside it that no child
    /// claims, so whoever clicked there left nothing focused. Given back,
    /// the root left Back, a closing modal, File history and Blame on
    /// nothing a person could see, where Down moved nothing.
    pub(super) fn saved_focus(&self, focus: Option<FocusHandle>) -> Option<FocusHandle> {
        focus.filter(|focus| *focus != self.app_focus)
    }

    /// Where focus returns as a modal closes: `previous` while it is still
    /// drawn, else the shown mode's own list, which the window then names
    /// as its pane (`Pane::History` or `Pane::Files`, as choosing a commit
    /// or a file does), else the window's root. The pane once stayed the one
    /// named before the modal opened, so a modal opened from History's
    /// changed files that closed onto its commit list left the window
    /// naming the file list, which a File history or Blame opened next
    /// keeps and restores as it closes. A `previous` that is the window's
    /// root counts as none (`saved_focus`).
    fn modal_return_target(
        &mut self,
        previous: Option<&FocusHandle>,
        window: &Window,
    ) -> FocusHandle {
        // In this window every attached control inherits Root's key context,
        // including sibling popup menus outside app_focus. A missing/detached
        // handle resolves to GPUI's synthetic dispatch root with no contexts.
        // Comparing a handle with itself cannot establish attachment.
        if let Some(previous) = self
            .saved_focus(previous.cloned())
            .filter(|focus| self.app_focus.contains(focus, window))
        {
            return previous;
        }
        if self.page != AppPage::Repository || !self.app_focus.contains(self.mode_list().0, window)
        {
            return self.app_focus.clone();
        }
        self.take_mode_list()
    }

    /// The shown mode's own list and the pane it is: History's commit list,
    /// else the changed-file list of Compare or Changes.
    fn mode_list(&self) -> (&FocusHandle, Pane) {
        if self.mode == WorkspaceMode::History {
            (&self.focus, Pane::History)
        } else {
            (&self.file_focus, Pane::Files)
        }
    }

    /// The shown mode's own list, for focus to land on, with the window
    /// naming the pane it is, as choosing a commit or a file names it.
    pub(super) fn take_mode_list(&mut self) -> FocusHandle {
        let (list, pane) = self.mode_list();
        let list = list.clone();
        self.pane = pane;
        list
    }

    /// Where a modal that set `modal_return_focus` as it opened returns focus
    /// as it closes, so it can do so before the next frame's repair, which
    /// returns it to the same place: no frame leaves focus on the window's
    /// root, where nothing shows it, and none names a pane other than the
    /// list it falls back to.
    pub(super) fn modal_return(&mut self, window: &Window) -> FocusHandle {
        self.modal_return_target(self.modal_return_focus.clone().as_ref(), window)
    }

    pub(super) fn back_label(&self) -> &'static str {
        match self.page {
            AppPage::Settings => {
                match return_page(self.page, self.page_origin, self.repository.is_some()) {
                    AppPage::Projects => "Back to Projects",
                    _ => "Back to repository",
                }
            }
            AppPage::Projects => "Back to repository",
            AppPage::Repository if self.blame.is_visible() => "Back from Blame",
            AppPage::Repository if self.file_history.is_active() => "Back from File History",
            AppPage::Repository if self.revision_inspection.is_active() => "Back to inspection",
            AppPage::Repository if self.mode != WorkspaceMode::History => "Back to History",
            AppPage::Repository => "Back to Projects",
        }
    }

    pub(super) fn return_from_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.page = return_page(self.page, self.page_origin, self.repository.is_some());
        if self.page == AppPage::Repository {
            self.resume_repository_page(window, cx);
            // Leaving ran the read a preparation it cancelled had noted
            // (`show_settings`, `show_projects`); one still noted runs now,
            // and then the quiet refresh the page held back.
            self.read_deferred_working(window, cx);
        } else {
            self.app_focus.focus(window, cx);
        }
        self.repaint_page(window, cx);
    }

    /// Takes up the repository page as the window returns to it from another
    /// page, by Back or through the repository's own tab
    /// (`switch_repository_tab`): File history loads its selected revision
    /// where its page landed while the repository was hidden, which loads
    /// none, an editor for a preview that landed then is built, focus
    /// returns where it was as the window left, and quiet reads held
    /// meanwhile start, a write's reload among them.
    pub(super) fn resume_repository_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.resume_file_history(window, cx);
        // Returning activates nothing: a conflict's view whose preview landed
        // meanwhile opens without its draft taking focus, which returns
        // below to where it was; the draft took it for one frame first.
        if self.mode != WorkspaceMode::History {
            self.prepare_editor(false, window, cx);
            // A comparison restored with its tab whose reply landed
            // meanwhile restores its documents with its editor, as one
            // landing on this page does; one still being read keeps its
            // bookmark for its reply. Landed under Settings or Projects, its
            // documents were never restored, and the bookmark waited for
            // some later preview. Only that comparison's reply restores
            // them (`restore_tab_documents`): restored whenever nothing was
            // loading, another tab's places reached the comparison shown.
            if self.loading != Some(RESTORED_PREVIEW_LABEL) && self.content.is_some() {
                self.restore_tab_documents(cx);
            }
        }
        // With nothing focused as the window left, focus lands on the shown
        // mode's own list, which the window names as its pane. Picked here,
        // the list reaches `return_tab_focus` as the control to return to,
        // which names a pane only for a control the page no longer draws:
        // Back from Settings opened with nothing focused once put focus on
        // History's commit list with the window still naming the file list.
        // The window's root, which a click on an empty spot focuses, counts
        // as nothing (`saved_focus`): given back, it left the page with no
        // ring and Down moving nothing.
        let saved = self.page_return_focus.take();
        let focus = match self.saved_focus(saved) {
            Some(focus) => focus,
            None => self.take_mode_list(),
        };
        // The page draws the focus it returns to with its ring, as Settings
        // marks the control it opens on, whether a key or a click went
        // back: "Back to repository" pressed with the pointer ended the
        // ring, so the list it landed on showed none.
        self.focus_ring.mark(window);
        let path = self.path.clone();
        let owner = cx.entity().downgrade();
        // Focus goes back as the next frame starts, before it draws the page;
        // whether that frame draws the control is known only as it ends
        // (`give_page_focus_back`).
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.page == AppPage::Repository && this.path == path {
                    this.give_page_focus_back(focus, window, cx);
                    window.refresh();
                }
            });
        });
        self.try_automatic_refresh(window, cx);
    }

    pub(super) fn navigate_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        if self.page != AppPage::Repository {
            self.return_from_page(window, cx);
        } else if self.mode == WorkspaceMode::History
            && !self.blame.is_visible()
            && !self.file_history.is_active()
            && !self.revision_inspection.is_active()
        {
            self.show_projects(window, cx);
        } else {
            self.back_to_history(window, cx);
            self.repaint_page(window, cx);
        }
    }

    pub(super) fn repaint_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A page replaces the entire element tree. Request a full native frame,
        // including after layout, so the old workspace cannot remain painted.
        cx.notify();
        window.refresh();
        window.on_next_frame(|window, _| window.refresh());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn settings_returns_to_the_page_it_was_opened_from() {
        assert_eq!(
            return_page(AppPage::Settings, AppPage::Projects, true),
            AppPage::Projects
        );
        assert_eq!(
            return_page(AppPage::Settings, AppPage::Repository, true),
            AppPage::Repository
        );
        assert_eq!(
            return_page(AppPage::Settings, AppPage::Repository, false),
            AppPage::Projects
        );
    }

    /// While a write runs, Ctrl+2 and the header's Changes toggle open
    /// Working Changes, as Ctrl+1 and the History toggle return to History,
    /// and the command palette and the Linux Main Menu offer both: switching
    /// pages only reads. Changes then refuses its write controls at once, as
    /// the write's busy look has them, and asks for no status read of its
    /// own, which would only wait behind the write; the write's own read
    /// lands its rows when it ends. Each switch to Changes once left History
    /// in place from a toggle drawn and announced available. Projects, whose
    /// every action starts another operation, still waits: Ctrl+Shift+O
    /// leaves the page, and the header's Projects and History's Back to
    /// Projects, once they look refused, say why in their tooltips.
    #[gpui::test]
    async fn page_switches_stay_available_while_a_write_runs(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::views::tests::{draw, fixture_git, history_script, history_window_from, settle};
        use crate::workspace::{BUSY_REASON, refusals};
        let (fixture, app, cx) = history_window_from(cx, history_script(2, 3)).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        std::fs::write(root.join("draft.txt"), "draft\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        let changes = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.page == AppPage::Repository && app.mode == WorkspaceMode::Working
            })
        };
        let history = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.page == AppPage::Repository && app.mode == WorkspaceMode::History
            })
        };
        assert!(history(cx));
        assert_eq!(
            app.read_with(cx, |app, _| app
                .work_status
                .as_ref()
                .map(|s| s.entries.len())),
            Some(1),
            "Changes · 1"
        );

        // A write held open, as a Fetch from an unreachable remote stays.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.operation_busy.is_some()));
        let primary = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };

        cx.simulate_keystrokes(&format!("{primary}-2"));
        draw(cx);
        assert!(changes(cx), "Ctrl+2 opens Changes while a write runs");
        assert!(
            cx.update(|window, cx| app.read(cx).file_focus.is_focused(window)),
            "focus moves to the Changes list"
        );
        assert!(
            app.read_with(cx, |app, _| app.status_task.is_none()),
            "no status read waits behind the write"
        );
        assert_eq!(
            refusals::get("stage-all").map(|refusal| refusal.announced),
            Some(true),
            "Stage all refuses at once"
        );
        let tooltip = |key| refusals::tooltip(key).map(|tooltip| tooltip.to_string());
        let projects = projects_tooltip(None);
        assert_eq!(
            tooltip("projects"),
            Some(projects.clone()),
            "Projects at first"
        );

        cx.simulate_keystrokes(&format!("{primary}-1"));
        draw(cx);
        assert!(history(cx), "Ctrl+1 returns to History");
        let toggle = cx.debug_bounds("changes-tab").expect("the Changes toggle");
        cx.simulate_click(toggle.center(), Modifiers::default());
        draw(cx);
        assert!(changes(cx), "the Changes toggle opens Changes");
        let toggle = cx.debug_bounds("history-tab").expect("the History toggle");
        cx.simulate_click(toggle.center(), Modifiers::default());
        draw(cx);
        assert!(history(cx), "the History toggle returns to History");
        for (page, reason) in [
            (CommandId::History, None),
            (CommandId::Changes, None),
            (CommandId::Settings, None),
            (CommandId::Projects, Some(BUSY_REASON)),
        ] {
            assert_eq!(
                cx.update(|window, cx| app.read(cx).menu_command_reason(page, window, cx)),
                reason,
                "the palette's {page:?}"
            );
        }

        cx.simulate_keystrokes(&format!("{primary}-shift-o"));
        draw(cx);
        assert!(history(cx), "Projects waits for the write");
        let shortcut = format!("{}[", primary_label());
        assert_eq!(
            tooltip("page-back"),
            Some(format!("Back to Projects · {shortcut}")),
            "Back at first"
        );
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        draw(cx);
        assert_eq!(
            tooltip("page-back"),
            Some(format!("Back to Projects · {BUSY_REASON} · {shortcut}")),
            "Back to Projects once the write looks busy"
        );

        // The write's own read lands Changes' rows: the draft is staged.
        cx.simulate_keystrokes(&format!("{primary}-2"));
        draw(cx);
        assert!(changes(cx), "Ctrl+2 once the write looks busy");
        assert_eq!(
            tooltip("projects"),
            Some(projects_tooltip(Some(BUSY_REASON))),
            "Projects once the write looks busy"
        );
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        settle(&app, cx).await;
        app.update(cx, |app, _| app.file_paths.executor().drain());
        settle(&app, cx).await;
        assert!(changes(cx));
        assert_eq!(
            app.read_with(cx, |app, _| app.working_paths.staged_count),
            1,
            "the draft lands staged"
        );
    }

    /// The fixture `history_window_from` imports, checked out with
    /// `draft.txt` new beside it and its working state read, in History.
    async fn checked_out(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut VisualTestContext,
        std::path::PathBuf,
    ) {
        use crate::views::tests::{fixture_git, history_script, history_window_from, settle};
        let (fixture, app, cx) = history_window_from(cx, history_script(2, 3)).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        std::fs::write(root.join("draft.txt"), "draft\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let root = app.read_with(cx, |app, _| app.path.clone()).unwrap();
        (fixture, app, cx, root)
    }

    /// The header's Projects tooltip: its name, `reason` while it waits,
    /// then its shortcut, as every tab tooltip ends.
    fn projects_tooltip(reason: Option<&str>) -> String {
        let key = crate::shortcuts::label(crate::shortcuts::ShortcutId::Projects);
        match reason {
            Some(reason) => format!("Open Projects · {reason} · {key}"),
            None => format!("Open Projects · {key}"),
        }
    }

    fn primary(keys: &str) -> String {
        let primary = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        format!("{primary}-{keys}")
    }

    /// The working state's paths, sorted.
    fn entries(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
        app.read_with(cx, |app, _| {
            app.work_status.as_ref().map(|status| {
                let mut paths: Vec<_> = status
                    .entries
                    .iter()
                    .map(|entry| entry.path.display().to_string())
                    .collect();
                paths.sort();
                paths
            })
        })
    }

    /// The path of the file whose preview is shown.
    fn previewed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Option<String> {
        app.read_with(cx, |app, _| {
            app.content.as_ref()?;
            Some(app.files.first()?.path().display().to_string())
        })
    }

    /// Lets the quiet reads the window asked for land: they run on the
    /// window's reader, a thread `run_until_parked` cannot wait for.
    fn quiet_reads_land(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        for _ in 0..8 {
            if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                crate::path_filter::filters_land(app, cx);
                return;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
        }
        panic!("a quiet read asked for never ran");
    }

    /// Whether the status read Changes' Refresh asked for runs, or is noted
    /// to run as a write or a preparation ends. `working_read_pending` also
    /// counts a quiet read, such as the opening rescan, which runs on the
    /// window's reader that `settle` does not wait for and cannot start off
    /// the repository page: on a loaded host these tests once read it as
    /// Refresh's read left noted.
    fn status_read_pending(app: &GitTurtle) -> bool {
        app.status_task.is_some() || app.working_read_deferred.is_some()
    }

    /// Opening Working Changes while a write runs, in the write's tab or
    /// another, queues no read behind the write: the read waits for the write
    /// to end, which then reads the shown tab's working state whichever tab
    /// it wrote in. Each Primary+2 in another tab once queued a read on the
    /// one operation executor behind a write in the first, and with its
    /// bounded queue full the next was refused as busy, which emptied that
    /// tab's rows with "Background executor is busy" until something read
    /// again; the write's end read only its own tab. A repository opened in
    /// a new tab meanwhile says it is reading until then, not that its
    /// changes are unavailable.
    #[gpui::test]
    async fn changes_opened_in_another_tab_during_a_write_reads_once_it_ends(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, fixture_git, fixture_repository, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        let other = fixture_repository(fixture.path().join("other"))
            .path()
            .to_owned();
        std::fs::write(other.join("notes.txt"), "notes\n").unwrap();
        fixture_git(&other, &["add", "notes.txt"]);
        fixture_git(&other, &["commit", "--quiet", "-m", "Notes"]);
        std::fs::write(other.join("draft.txt"), "draft\n").unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(other.clone(), window, cx)
            })
        });
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.path.clone()),
            Some(other.clone())
        );
        assert_eq!(entries(&app, cx), Some(vec!["draft.txt".to_owned()]));
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle(&app, cx).await;

        // A write in the first tab held open, as a Fetch from an unreachable
        // remote stays, and the other tab shown meanwhile.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        draw(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.path.clone()),
            Some(other.clone())
        );
        assert!(app.read_with(cx, |app, _| app.operation_busy.is_some()));
        std::fs::write(other.join("later.txt"), "later\n").unwrap();
        for _ in 0..10 {
            cx.simulate_keystrokes(&primary("1"));
            draw(cx);
            cx.simulate_keystrokes(&primary("2"));
            draw(cx);
        }
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Working);
            assert_eq!(app.operation_error, None, "no read was refused as busy");
            assert!(app.status_task.is_none(), "no read waits behind the write");
            assert!(!app.working_rows.is_empty(), "the tab keeps its rows");
        });

        // Held behind the write, the read its end asks for waits until
        // looked at.
        let reads = app.update(cx, |app, _| app.operations.hold());
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        assert!(
            app.read_with(cx, |app, _| app.status_task.is_some()),
            "the write's end reads the shown tab's working state"
        );
        drop(reads);
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_ref(), Some(&other));
            assert_eq!(app.operation_error, None);
            assert!(!app.working_rows.is_empty());
        });
        assert_eq!(
            entries(&app, cx),
            Some(vec!["draft.txt".to_owned(), "later.txt".to_owned()]),
            "the rows read after the write"
        );

        // A repository opened in a new tab while a write runs in another:
        // its Changes says it is reading until the write ends, when it reads.
        let third = fixture_repository(fixture.path().join("third"))
            .path()
            .to_owned();
        std::fs::write(third.join("notes.txt"), "notes\n").unwrap();
        fixture_git(&third, &["add", "notes.txt"]);
        fixture_git(&third, &["commit", "--quiet", "-m", "Notes"]);
        std::fs::write(third.join("draft.txt"), "draft\n").unwrap();
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(third.clone(), window, cx)
            })
        });
        settle(&app, cx).await;
        cx.simulate_keystrokes(&primary("2"));
        draw(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_ref(), Some(&third));
            assert!(app.operation_busy.is_some());
            assert!(app.work_status.is_none(), "its read waits for the write");
            assert!(app.status_task.is_none());
        });
        assert!(
            cx.debug_bounds("working-reading").is_some(),
            "Changes says it is reading, not that it is unavailable"
        );
        let reads = app.update(cx, |app, _| app.operations.hold());
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        assert!(app.read_with(cx, |app, _| app.status_task.is_some()));
        drop(reads);
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        assert_eq!(entries(&app, cx), Some(vec!["draft.txt".to_owned()]));
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            None
        );
    }

    /// An LFS download reloads only the preview it was asked from: Changes
    /// opened while it ran reads once it ends, and in Changes whose view
    /// has moved on by then, here to another row, the row shown keeps its
    /// preview, which a working read at the download's end once cleared and
    /// loaded again, losing its scroll and selection. The download's busy
    /// spell and end are driven as `write_from` drives them
    /// (`read_after_write`), since building one needs Git LFS installed; a
    /// running write refuses a row's selection, so the row is chosen before
    /// that end.
    #[gpui::test]
    async fn an_lfs_downloads_end_reads_only_what_its_view_asked_for(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        use gitturtle_core::ChangeArea;
        let (fixture, app, cx, root) = checked_out(cx).await;
        // What `write_from` captures as a download starts.
        let capture = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.selected_file
                    .and_then(|index| app.files.get(index))
                    .cloned()
                    .map(|file| (file, app.mode, app.generation))
            })
        };
        let start = |cx: &mut VisualTestContext| {
            app.update(cx, |app, _| {
                app.operation_busy = Some("Downloading LFS object…");
                app.operation_repository = app.path.clone();
            })
        };
        let end = |cx: &mut VisualTestContext, selection: Option<crate::workspace::LfsView>| {
            let root = root.clone();
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.operation_busy = None;
                    app.operation_repository = None;
                    app.read_after_write(&root, false, Some((true, selection)), window, cx);
                })
            })
        };

        // From History, with the newest commit's file selected.
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        let selection = capture(cx);
        assert!(selection.is_some(), "a file is shown");
        // The download holds the operation executor while it runs.
        let download = app.update(cx, |app, _| app.operations.hold());
        start(cx);
        std::fs::write(fixture.path().join("narrow/later.txt"), "later\n").unwrap();
        cx.simulate_keystrokes(&primary("2"));
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working));
        assert!(
            app.read_with(cx, |app, _| app.status_task.is_none()),
            "no read waits behind the download"
        );
        let reads = app.update(cx, |app, _| app.operations.hold());
        drop(download);
        end(cx, selection);
        assert!(
            app.read_with(cx, |app, _| app.status_task.is_some()),
            "Changes, opened during the download, reads as it ends"
        );
        drop(reads);
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        assert_eq!(
            entries(&app, cx),
            Some(vec!["draft.txt".to_owned(), "later.txt".to_owned()])
        );

        // In Changes, a download asked from draft.txt's preview, with
        // later.txt shown by the time it ends.
        let select = |cx: &mut VisualTestContext, name: &str| {
            let index = app.read_with(cx, |app, _| {
                app.work_status
                    .as_ref()
                    .unwrap()
                    .entries
                    .iter()
                    .position(|entry| entry.path == std::path::Path::new(name))
                    .unwrap()
            });
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.select_working(index, ChangeArea::Unstaged, window, cx)
                })
            });
        };
        select(cx, "draft.txt");
        settle(&app, cx).await;
        assert_eq!(previewed(&app, cx).as_deref(), Some("draft.txt"));
        let selection = capture(cx);
        select(cx, "later.txt");
        settle(&app, cx).await;
        assert_eq!(previewed(&app, cx).as_deref(), Some("later.txt"));
        end(cx, selection);
        app.read_with(cx, |app, _| {
            assert!(app.status_task.is_none(), "no working read");
            assert!(app.task.is_none(), "no preview read");
        });
        assert_eq!(
            previewed(&app, cx).as_deref(),
            Some("later.txt"),
            "the shown row keeps its preview"
        );
    }

    /// A pointer resting on the header's Projects reads why it waits once a
    /// write looks busy, and its plain tooltip again once the write ends,
    /// without moving. The kit kept the tooltip built as the pointer entered,
    /// so resting there through a write still read "Open Projects" beside
    /// the dimmed button, and the reason after the write.
    #[gpui::test]
    async fn a_pointer_resting_on_projects_reads_why_it_waits_while_a_write_runs(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, BUSY_REASON};
        let (_fixture, app, cx, _) = checked_out(cx).await;
        // Wide enough that the busy look's progress label and Cancel take
        // none of the header's room from the controls beside Projects.
        cx.simulate_resize(size(px(1600.), px(900.)));
        // Projects stands in the header beside History's root, where Back to
        // Projects takes its place.
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        let shows = |cx: &mut VisualTestContext, text: &str| {
            cx.debug_bounds(format!("projects/tooltip/{text}").leak())
                .is_some()
        };
        let rest = projects_tooltip(None);
        let waiting = projects_tooltip(Some(BUSY_REASON));
        let projects = cx.debug_bounds("projects").expect("the header's Projects");
        cx.simulate_mouse_move(projects.center(), None, Modifiers::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(600));
        draw(cx);
        assert!(shows(cx, &rest), "at rest");

        // A write held open, as a Fetch from an unreachable remote stays.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        draw(cx);
        assert!(shows(cx, &rest), "through the busy look's delay");
        cx.executor().advance_clock(BUSY_LOOK_DELAY);
        draw(cx);
        assert_eq!(
            cx.debug_bounds("projects"),
            Some(projects),
            "Projects stays under the pointer"
        );
        assert!(shows(cx, &waiting), "once the write looks busy");
        assert!(!shows(cx, &rest));

        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.operation_busy.is_none()));
        draw(cx);
        assert!(shows(cx, &rest), "once the write ends");
        assert!(!shows(cx, &waiting));
    }

    /// History's Back to Projects says why it waits for a write between its
    /// name and its shortcut, the order every tooltip keeps: a shortcut ends
    /// it ("History · Primary+1") and a reason follows the name ("Open
    /// Projects · Wait for …"). It read "Back to Projects · Primary+[ ·
    /// Wait for …", the reason trailing the shortcut. Resting there, the
    /// pointer reads it lose the reason as the write ends.
    #[gpui::test]
    async fn back_to_projects_gives_its_reason_before_its_shortcut(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, BUSY_REASON};
        let (_fixture, app, cx, _) = checked_out(cx).await;
        cx.simulate_resize(size(px(1600.), px(900.)));
        draw(cx);
        let shows = |cx: &mut VisualTestContext, text: &str| {
            cx.debug_bounds(format!("page-back/tooltip/{text}").leak())
                .is_some()
        };
        let shortcut = format!("{}[", primary_label());
        let rest = format!("Back to Projects · {shortcut}");
        let waiting = format!("Back to Projects · {BUSY_REASON} · {shortcut}");

        // A write held open, as a Fetch from an unreachable remote stays.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        cx.executor().advance_clock(BUSY_LOOK_DELAY);
        draw(cx);
        let back = cx
            .debug_bounds("page-back")
            .expect("History's Back to Projects");
        cx.simulate_mouse_move(back.center(), None, Modifiers::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(600));
        draw(cx);
        assert!(shows(cx, &waiting), "while the write looks busy");

        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        settle(&app, cx).await;
        draw(cx);
        assert!(shows(cx, &rest), "once the write ends");
        assert!(!shows(cx, &waiting));
    }

    /// A review's preparation runs on the operation executor as a write
    /// does, so Changes opened while it runs queues no read behind it either:
    /// the preparation's end reads the working state. Each opening once
    /// queued a read behind it, as behind a write.
    #[gpui::test]
    async fn changes_opened_while_a_review_is_prepared_reads_once_it_ends(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        let draft = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .find(|entry| entry.path == std::path::Path::new("draft.txt"))
                .cloned()
                .unwrap()
        });
        // Discard…'s review, its preparation held as a slow disk holds it.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        assert!(app.read_with(cx, |app, _| app.operation_busy.is_some()));
        std::fs::write(fixture.path().join("narrow/later.txt"), "later\n").unwrap();
        for _ in 0..10 {
            cx.simulate_keystrokes(&primary("1"));
            draw(cx);
            cx.simulate_keystrokes(&primary("2"));
            draw(cx);
        }
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Working);
            assert_eq!(app.operation_error, None, "no read was refused as busy");
            assert!(app.status_task.is_none(), "no read waits behind it");
        });
        drop(held);
        // The preparation answers, and its end's read lands: nothing else
        // reads the working state here.
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        assert_eq!(
            entries(&app, cx),
            Some(vec!["draft.txt".to_owned(), "later.txt".to_owned()]),
            "the preparation's end reads the working state"
        );
    }

    /// A plan History asks for opens over History only. Primary + 2 while
    /// Amend last commit or a branch's actions are prepared opens Changes
    /// and drops the plan, as leaving the page does, while the preparation's
    /// end still reads the working state Changes asked for meanwhile, with
    /// no read queued behind it. Amend's form once opened over the working
    /// list. Asked for again in History, the plan opens there.
    #[gpui::test]
    async fn changes_opened_while_history_prepares_a_plan_drops_the_plan(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        let head = app.read_with(cx, |app, _| app.commits[0].oid.clone());
        let amend_open = |cx: &mut VisualTestContext| {
            app.read_with(cx, crate::recovery::amend_description)
                .is_some()
        };
        let dialog =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
        /// Returns once the preparation and the reads after it have landed.
        async fn ends(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
            app.read_with(cx, |app, _| app.operations.drain());
            settle(app, cx).await;
            crate::path_filter::filters_land(app, cx);
        }
        /// Starts a plan's preparation for the commit given.
        type Prepare = fn(&mut GitTurtle, String, &mut Window, &mut Context<GitTurtle>);
        let prepare: [(&str, Prepare); 2] = [
            ("Amend last commit", |app, head, window, cx| {
                crate::recovery::prepare_amend(app, head, window, cx)
            }),
            ("main's branch actions", |app, _, window, cx| {
                app.open_contextual_branch("main".into(), false, window, cx)
            }),
        ];
        for (index, (plan, prepare)) in prepare.into_iter().enumerate() {
            // Its preparation held as a slow disk holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            let head = head.clone();
            cx.update(|window, cx| app.update(cx, |app, cx| prepare(app, head, window, cx)));
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_some()),
                "{plan} is prepared"
            );
            let later = format!("later-{index}.txt");
            std::fs::write(fixture.path().join("narrow").join(&later), "later\n").unwrap();
            cx.simulate_keystrokes(&primary("2"));
            draw(cx);
            app.read_with(cx, |app, _| {
                assert!(app.mode == WorkspaceMode::Working, "{plan}: Changes opens");
                assert!(app.status_task.is_none(), "{plan}: no read waits behind it");
            });
            drop(held);
            ends(&app, cx).await;
            assert!(!amend_open(cx), "{plan}: no Amend form over Changes");
            assert!(!dialog(cx), "{plan}: no plan over Changes");
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_none()),
                "{plan}: the preparation ended"
            );
            assert!(
                entries(&app, cx).is_some_and(|entries| entries.contains(&later)),
                "{plan}: the preparation's end reads the working state"
            );
            cx.simulate_keystrokes(&primary("1"));
            draw(cx);
        }

        // Asked for in History and answered there, the plan opens.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                crate::recovery::prepare_amend(app, head, window, cx)
            })
        });
        ends(&app, cx).await;
        assert!(amend_open(cx), "Amend last commit opens in History");
    }

    /// The plans History asks for whose preparation can be held: Amend last
    /// commit, main's branch actions and, last as it leaves the repository
    /// mid-rebase, a paused rebase's Continue (`pause_a_reword`).
    type Prepare = fn(&mut GitTurtle, String, &mut Window, &mut Context<GitTurtle>);
    fn plans() -> [(&'static str, Prepare); 3] {
        [
            ("Amend last commit", |app, head, window, cx| {
                crate::recovery::prepare_amend(app, head, window, cx)
            }),
            ("main's branch actions", |app, _, window, cx| {
                app.open_contextual_branch("main".into(), false, window, cx)
            }),
            ("Continue rebase", |app, _, window, cx| {
                app.prepare_interactive_rebase_continue(window, cx)
            }),
        ]
    }

    /// Pauses a reword of `root`'s last commit for its message, which
    /// Continue rebase edits, and reads the paused state.
    async fn pause_a_reword(
        app: &Entity<GitTurtle>,
        root: &std::path::Path,
        cx: &mut VisualTestContext,
    ) {
        use crate::views::tests::{fixture_git, settle};
        use gitturtle_core::{InteractiveRebaseCommand, RebaseAction, WriteCommand};
        fixture_git(root, &["config", "user.name", "Fixture"]);
        fixture_git(root, &["config", "user.email", "fixture@example.invalid"]);
        fixture_git(root, &["config", "commit.gpgsign", "false"]);
        let repo = app.read_with(cx, |app, _| app.repository.clone().unwrap());
        let plan = repo.interactive_rebase_plan("HEAD~1").unwrap();
        let mut steps = plan.steps();
        steps[0].action = RebaseAction::Reword;
        repo.execute(&WriteCommand::InteractiveRebase(Arc::new(
            InteractiveRebaseCommand::Start {
                plan,
                steps,
                acknowledge_published: false,
            },
        )))
        .unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(app, cx).await;
        crate::path_filter::filters_land(app, cx);
        assert!(app.read_with(cx, |app, _| app.integration_state.is_some()));
    }

    /// Lets a held preparation end, and the reads after it land.
    async fn preparation_ends(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(app, cx).await;
        crate::path_filter::filters_land(app, cx);
    }

    fn dialog_open(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.has_active_dialog(cx))
    }

    fn close_dialogs(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.close_all_dialogs(cx));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        crate::views::tests::draw(cx);
    }

    /// A plan Changes dropped as it opened leaves the next one: Changes'
    /// own Amend last commit…, its branch picker's actions or the paused
    /// rebase's Continue, asked for there once History's had ended unseen,
    /// opens its plan over Changes. The preparation's start resets what the
    /// last one dropped (`cancel_recovery_read`, `cancel_branch_action`,
    /// `cancel_interactive_rebase_action`), or the next plan would be
    /// dropped too, with nothing said. Continue rebase's dialog is dropped as
    /// Changes opens, as the other plans are
    /// (`changes_opened_while_history_prepares_a_plan_drops_the_plan`).
    #[gpui::test]
    async fn a_plan_changes_dropped_leaves_the_next_one_asked_for_there(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        let (_fixture, app, cx, root) = checked_out(cx).await;
        let head = app.read_with(cx, |app, _| app.commits[0].oid.clone());
        for (plan, prepare) in plans() {
            if plan == "Continue rebase" {
                pause_a_reword(&app, &root, cx).await;
            }
            // Asked for in History, held, and Changes opened.
            let held = app.update(cx, |app, _| app.operations.hold());
            let commit = head.clone();
            cx.update(|window, cx| app.update(cx, |app, cx| prepare(app, commit, window, cx)));
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_some()),
                "{plan} is prepared"
            );
            cx.simulate_keystrokes(&primary("2"));
            draw(cx);
            drop(held);
            preparation_ends(&app, cx).await;
            assert!(
                app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working
                    && app.operation_busy.is_none()),
                "{plan}: the preparation ended in Changes"
            );
            assert!(
                !dialog_open(cx),
                "{plan}: History's plan opens nothing over Changes"
            );
            // Asked for again in Changes, the plan opens there.
            let commit = head.clone();
            cx.update(|window, cx| app.update(cx, |app, cx| prepare(app, commit, window, cx)));
            preparation_ends(&app, cx).await;
            assert!(
                dialog_open(cx),
                "{plan}: asked for in Changes, it opens there"
            );
            close_dialogs(cx);
            cx.simulate_keystrokes(&primary("1"));
            draw(cx);
        }
    }

    /// History shown again before the plan Changes dropped is prepared
    /// wants it again: Primary + 2 and Primary + 1 while Amend last commit,
    /// a branch's actions or a paused rebase's Continue are prepared, and
    /// the plan opens over History as its preparation ends. It once stayed
    /// dropped, and the action did nothing.
    #[gpui::test]
    async fn a_plan_changes_dropped_opens_once_history_shows_again(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        let (_fixture, app, cx, root) = checked_out(cx).await;
        let head = app.read_with(cx, |app, _| app.commits[0].oid.clone());
        for (plan, prepare) in plans() {
            if plan == "Continue rebase" {
                pause_a_reword(&app, &root, cx).await;
            }
            let held = app.update(cx, |app, _| app.operations.hold());
            let commit = head.clone();
            cx.update(|window, cx| app.update(cx, |app, cx| prepare(app, commit, window, cx)));
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_some()),
                "{plan} is prepared"
            );
            cx.simulate_keystrokes(&primary("2"));
            draw(cx);
            assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working));
            cx.simulate_keystrokes(&primary("1"));
            draw(cx);
            drop(held);
            preparation_ends(&app, cx).await;
            assert!(
                app.read_with(cx, |app, _| app.mode == WorkspaceMode::History
                    && app.operation_busy.is_none()),
                "{plan}: the preparation ended in History"
            );
            assert!(
                dialog_open(cx),
                "{plan}: back in History, its plan opens there"
            );
            close_dialogs(cx);
        }
    }

    /// A plan Changes asks for opens over Changes only: Amend last
    /// commit…, main's branch actions or a paused rebase's Continue asked
    /// for there, then Primary + 1, and History drops it, as it drops
    /// Changes' Discard…, keeping the busy state until its read ends, whose
    /// end reads the working state Refresh asked for meanwhile. The plan
    /// recorded no view that asked for it, so History shown again wanted it
    /// as one of its own, and its form opened over History.
    #[gpui::test]
    async fn a_plan_changes_asked_for_opens_nothing_over_history(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, root) = checked_out(cx).await;
        let head = app.read_with(cx, |app, _| app.commits[0].oid.clone());
        for (index, (plan, prepare)) in plans().into_iter().enumerate() {
            if plan == "Continue rebase" {
                pause_a_reword(&app, &root, cx).await;
            }
            cx.simulate_keystrokes(&primary("2"));
            settle(&app, cx).await;
            crate::path_filter::filters_land(&app, cx);
            // Asked for in Changes, held as a slow disk holds it, and
            // Refresh asked for meanwhile.
            let held = app.update(cx, |app, _| app.operations.hold());
            let commit = head.clone();
            cx.update(|window, cx| app.update(cx, |app, cx| prepare(app, commit, window, cx)));
            let label = app.read_with(cx, |app, _| app.operation_busy);
            assert!(label.is_some(), "{plan} is prepared");
            let later = format!("later-{index}.txt");
            std::fs::write(root.join(&later), "later\n").unwrap();
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));

            cx.simulate_keystrokes(&primary("1"));
            draw(cx);
            app.read_with(cx, |app, _| {
                assert!(app.mode == WorkspaceMode::History, "{plan}: History shows");
                assert_eq!(
                    app.operation_busy, label,
                    "{plan}: busy until its read ends"
                );
                assert!(
                    app.status_task.is_none(),
                    "{plan}: no read queues behind it"
                );
            });
            drop(held);
            preparation_ends(&app, cx).await;
            assert!(!dialog_open(cx), "{plan}: nothing opens over History");
            assert!(
                app.read_with(cx, |app, _| app.mode == WorkspaceMode::History),
                "{plan}: History stays"
            );
            app.read_with(cx, |app, _| {
                assert_eq!(app.operation_busy, None, "{plan}: the busy state ends");
                assert!(!status_read_pending(app), "{plan}: no read stays noted");
            });
            assert!(
                entries(&app, cx).is_some_and(|entries| entries.contains(&later)),
                "{plan}: the read Refresh asked for runs"
            );
            close_dialogs(cx);
        }
    }

    /// A Commit or Fetch refreshes quietly as it ends, and that quiet read
    /// serves Changes opened while it ran: its rows land without a second,
    /// aloud read clearing them, and the selected row gets back the preview
    /// Changes cleared as it opened (`worker::QuietPreview::Changed`).
    #[gpui::test]
    async fn a_commits_quiet_refresh_serves_changes_opened_while_it_ran(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, fixture_git, settle};
        use gitturtle_core::ChangeArea;
        let (fixture, app, cx, _) = checked_out(cx).await;
        let root = fixture.path().join("narrow");
        std::fs::write(root.join("notes.txt"), "rewritten\n").unwrap();
        fixture_git(&root, &["add", "notes.txt"]);
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        assert_eq!(
            entries(&app, cx),
            Some(vec!["draft.txt".to_owned(), "notes.txt".to_owned()])
        );
        let draft = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .position(|entry| entry.path == std::path::Path::new("draft.txt"))
                .unwrap()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_working(draft, ChangeArea::Unstaged, window, cx)
            })
        });
        settle(&app, cx).await;
        assert_eq!(previewed(&app, cx).as_deref(), Some("draft.txt"));
        cx.simulate_keystrokes(&primary("1"));
        draw(cx);

        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::Commit {
                        message: "Record notes".into(),
                    },
                    "Creating commit…",
                    window,
                    cx,
                )
            })
        });
        cx.simulate_keystrokes(&primary("2"));
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Working);
            assert!(app.status_task.is_none(), "no read waits behind the commit");
        });
        assert_eq!(
            previewed(&app, cx),
            None,
            "Changes clears the preview as it opens"
        );
        // Held behind the commit, an aloud read its end asked for would wait
        // until looked at.
        let reads = app.update(cx, |app, _| app.operations.hold());
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the commit");
        write.await;
        assert!(
            app.read_with(cx, |app, _| app.status_task.is_none()),
            "no aloud read after the commit"
        );
        drop(reads);
        quiet_reads_land(&app, cx);
        draw(cx);
        assert_eq!(entries(&app, cx), Some(vec!["draft.txt".to_owned()]));
        assert_eq!(
            previewed(&app, cx).as_deref(),
            Some("draft.txt"),
            "the selected row's preview is back"
        );
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            None
        );
    }

    /// The quiet read a Commit's end starts reads the working state, so
    /// Changes with no status shown says it is reading while that read runs,
    /// in its list and in the commit action, as for a status read. It said
    /// "Changes unavailable", and "Refresh to read the working tree again",
    /// until the read landed.
    #[gpui::test]
    async fn changes_says_it_reads_while_a_commits_quiet_refresh_runs(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, fixture_git, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        let root = fixture.path().join("narrow");
        std::fs::write(root.join("notes.txt"), "rewritten\n").unwrap();
        fixture_git(&root, &["add", "notes.txt"]);
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        // Nothing may read the working state before the Commit. The rescan
        // the window asks for as its watch registers, restarted by each
        // read before it lands, runs again as Changes' status lands: still
        // running, it says Changes is reading, and landing later, it reads
        // back the status cleared below.
        quiet_reads_land(&app, cx);
        // No status shown, as a status read that failed leaves Changes.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.work_status = None;
                app.working_paths.reset();
                app.working_rows.clear();
                app.working_selected = None;
                app.follow_working_menu(window, cx);
                cx.notify();
            })
        });
        draw(cx);
        assert!(cx.debug_bounds("working-unavailable").is_some());

        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::Commit {
                        message: "Record notes".into(),
                    },
                    "Creating commit…",
                    window,
                    cx,
                )
            })
        });
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the commit");
        write.await;
        // One frame as the quiet read runs, before its reply can land.
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        app.read_with(cx, |app, _| {
            assert!(app.work_status.is_none(), "nothing has read it yet");
            assert!(app.status_task.is_none(), "no status read after the commit");
        });
        assert!(
            cx.debug_bounds("working-reading").is_some(),
            "Changes says it is reading"
        );
        assert!(cx.debug_bounds("working-unavailable").is_none());
        assert_eq!(
            app.read_with(cx, |app, cx| app.commit_action_label(cx)),
            "Reading working changes…"
        );

        quiet_reads_land(&app, cx);
        draw(cx);
        assert_eq!(entries(&app, cx), Some(vec!["draft.txt".to_owned()]));
        assert!(cx.debug_bounds("working-reading").is_none());
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            None
        );
    }

    /// A create that opens its repository leaves a working read noted for
    /// the repository it leaves to that `open`, which drops it, as its
    /// snapshot reads the new repository afresh: the create's end reads
    /// nothing there itself. Run at the end, the read cleared Changes'
    /// preview just before the tab was kept, and the tab came back without
    /// it. Nothing notes such a read while Projects shows today, so the test
    /// notes it, as a route that did would leave it.
    #[gpui::test]
    async fn a_create_that_opens_its_repository_leaves_the_noted_read_to_it(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::settle;
        let (fixture, app, cx, root) = checked_out(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        quiet_reads_land(&app, cx);
        select_unstaged(&app, "draft.txt", cx).await;
        assert_eq!(previewed(&app, cx).as_deref(), Some("draft.txt"));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
        // Create, held as a slow disk holds it, and a read noted meanwhile.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.project_event(
                    &crate::projects::ProjectEvent::Create {
                        destination: fixture.path().join("created"),
                        branch: "main".into(),
                    },
                    window,
                    cx,
                )
            })
        });
        app.update(cx, |app, _| {
            assert!(app.operation_busy.is_some(), "the create runs");
            app.working_read_deferred = app.path.clone();
        });
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_busy, None, "the create ended");
            assert!(
                app.path.as_ref().is_some_and(|path| path != &root),
                "it opened the created repository"
            );
            assert_eq!(app.working_read_deferred, None, "the open dropped the read");
        });
        // The first tab, shown again, keeps the preview it had.
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        assert_eq!(app.read_with(cx, |app, _| app.path.clone()), Some(root));
        assert_eq!(
            previewed(&app, cx).as_deref(),
            Some("draft.txt"),
            "the tab keeps its preview"
        );
    }

    /// A quiet read that attribution holds back waits for attribution to
    /// close, so Changes with no status shown says it is unavailable, not
    /// reading, until then, and closing attribution starts the read. With
    /// attribution open on a row, a Refresh whose status read failed and a
    /// change reported meanwhile kept Changes saying "Reading the working
    /// tree…" for as long as attribution stayed open, though nothing read,
    /// and closing it read nothing until another change was reported.
    #[gpui::test]
    async fn a_quiet_read_attribution_holds_back_waits_for_it_to_close(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, root) = checked_out(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        quiet_reads_land(&app, cx);
        select_unstaged(&app, "draft.txt", cx).await;
        let row = app.read_with(cx, |app, _| app.working_selected.unwrap().0);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(row, window, cx)));
        crate::blame::lands(&app, cx);
        assert!(app.read_with(cx, |app, _| app.blame.is_visible()));
        // Refresh, its status read failing on an unreadable index.
        let index = root.join(".git").join("index");
        let kept = std::fs::read(&index).expect("the fixture's index");
        std::fs::write(&index, b"not an index").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        // A change reported meanwhile, as the watcher reports one.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_automatic_refresh(
                    crate::local_refresh::LocalChange {
                        worktree: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        });
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.work_status.is_none(), "the status read failed");
            assert!(app.blame.is_visible(), "attribution stays open");
            assert!(app.status_task.is_none());
            assert!(
                app.automatic.quiet_read_requested(),
                "a quiet read waits for attribution"
            );
        });
        assert!(
            cx.debug_bounds("working-unavailable").is_some(),
            "Changes says it is unavailable"
        );
        assert!(cx.debug_bounds("working-reading").is_none());
        assert_eq!(
            app.read_with(cx, |app, cx| app.commit_action_label(cx)),
            "Changes unavailable"
        );
        // The index readable again and attribution closed: the next frame
        // starts the read, drawn before its reply can land, and Changes says
        // it reads. Nothing else would start it (`quiet_reads_land`).
        std::fs::write(&index, kept).unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.close_blame(window, cx);
            });
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        assert!(
            cx.debug_bounds("working-reading").is_some(),
            "Changes says it is reading"
        );
        quiet_reads_land(&app, cx);
        draw(cx);
        assert_eq!(entries(&app, cx), Some(vec!["draft.txt".to_owned()]));
    }

    /// A second repository, `other` beside `checked_out`'s, with `draft.txt`
    /// new, opened in a tab of its own, shown, and read there.
    async fn another_tab(
        fixture: &tempfile::TempDir,
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) -> std::path::PathBuf {
        use crate::views::tests::{fixture_git, fixture_repository, settle};
        let other = fixture_repository(fixture.path().join("other"))
            .path()
            .to_owned();
        std::fs::write(other.join("notes.txt"), "notes\n").unwrap();
        fixture_git(&other, &["add", "notes.txt"]);
        fixture_git(&other, &["commit", "--quiet", "-m", "Notes"]);
        std::fs::write(other.join("draft.txt"), "draft\n").unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(other.clone(), window, cx)
            })
        });
        settle(app, cx).await;
        assert_eq!(entries(app, cx), Some(vec!["draft.txt".to_owned()]));
        other
    }

    /// `checked_out`'s repository in the first tab, shown, and a second
    /// repository, `other`, with `draft.txt` new, opened in a second tab and
    /// read there.
    async fn beside_another_tab(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut VisualTestContext,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        use crate::views::tests::settle;
        let (fixture, app, cx, root) = checked_out(cx).await;
        let other = another_tab(&fixture, &app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.path.clone()),
            Some(root.clone())
        );
        (fixture, app, cx, root, other)
    }

    /// The shown working state's entry for `path`.
    fn entry(
        app: &Entity<GitTurtle>,
        path: &str,
        cx: &mut VisualTestContext,
    ) -> gitturtle_core::StatusEntry {
        app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .find(|entry| entry.path == std::path::Path::new(path))
                .cloned()
                .unwrap()
        })
    }

    /// Settings and Projects cancel a review's preparation, whose end then
    /// never runs, so they run the working-state read Changes asked for
    /// meanwhile themselves. Refresh pressed while Discard… reviewed a row,
    /// then Settings or Projects opened and the tab clicked back, which
    /// reads nothing of its own, once never ran: the rows stayed as they
    /// were, and the read stayed noted for a later preparation's end.
    #[gpui::test]
    async fn a_refresh_a_cancelled_preparation_held_back_still_runs(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        type Leave = fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>);
        let leave: [(&str, Leave); 2] = [
            ("Settings", |app, window, cx| app.show_settings(window, cx)),
            ("Projects", |app, window, cx| app.show_projects(window, cx)),
        ];
        for (index, (page, leave)) in leave.into_iter().enumerate() {
            // Discard…'s review, held as a slow disk holds it, and Refresh.
            let held = app.update(cx, |app, _| app.operations.hold());
            let draft = entry(&app, "draft.txt", cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
            let later = format!("later-{index}.txt");
            std::fs::write(fixture.path().join("narrow").join(&later), "later\n").unwrap();
            cx.simulate_keystrokes(&primary("r"));
            draw(cx);
            app.read_with(cx, |app, _| {
                assert!(app.operation_busy.is_some(), "{page}: the review runs");
                assert!(status_read_pending(app), "{page}: Refresh is asked for");
                assert!(app.status_task.is_none(), "{page}: Refresh waits for it");
            });
            cx.update(|window, cx| app.update(cx, |app, cx| leave(app, window, cx)));
            draw(cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
            });
            draw(cx);
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle(&app, cx).await;
            crate::path_filter::filters_land(&app, cx);
            app.read_with(cx, |app, _| {
                assert!(
                    app.page == AppPage::Repository && app.mode == WorkspaceMode::Working,
                    "{page}: the tab shows Changes again"
                );
                assert!(app.operation_busy.is_none(), "{page}");
                assert!(!status_read_pending(app), "{page}: no read stays noted");
            });
            assert!(
                entries(&app, cx).is_some_and(|entries| entries.contains(&later)),
                "{page}: Refresh ran"
            );
        }
    }

    /// Selects `path`'s Unstaged row in Changes and lets its preview land.
    async fn select_unstaged(app: &Entity<GitTurtle>, path: &str, cx: &mut VisualTestContext) {
        let index = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .position(|entry| entry.path == std::path::Path::new(path))
                .unwrap()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_working(index, gitturtle_core::ChangeArea::Unstaged, window, cx)
            })
        });
        crate::views::tests::settle(app, cx).await;
    }

    /// A status read that lands while Settings or Projects shows leaves
    /// focus on that page. Refresh pressed during Discard…'s review with a
    /// row selected, then Settings or Projects opened, which runs the read
    /// the review held back: as it landed it selected the row again for its
    /// preview, and focus moved from the control the page opened on to the
    /// hidden Changes list. Back still returns focus to the list.
    #[gpui::test]
    async fn a_status_read_landing_behind_settings_or_projects_leaves_focus_there(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let on_list = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).file_focus.is_focused(window))
        };
        type Leave = fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>);
        let leave: [(&str, AppPage, Leave); 2] = [
            ("Settings", AppPage::Settings, |app, window, cx| {
                app.show_settings(window, cx)
            }),
            ("Projects", AppPage::Projects, |app, window, cx| {
                app.show_projects(window, cx)
            }),
        ];
        for (index, (name, page, leave)) in leave.into_iter().enumerate() {
            select_unstaged(&app, "draft.txt", cx).await;
            assert!(on_list(cx), "{name}: the selected row's list holds focus");
            // Discard…'s review, held as a slow disk holds it, and Refresh.
            let held = app.update(cx, |app, _| app.operations.hold());
            let draft = entry(&app, "draft.txt", cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
            let later = format!("later-{index}.txt");
            std::fs::write(fixture.path().join("narrow").join(&later), "later\n").unwrap();
            cx.simulate_keystrokes(&primary("r"));
            draw(cx);
            assert!(app.read_with(cx, |app, _| status_read_pending(app)));
            cx.update(|window, cx| app.update(cx, |app, cx| leave(app, window, cx)));
            draw(cx);
            let opened = focused(cx);
            assert!(
                opened.is_some() && !on_list(cx),
                "{name} opens with its own focus"
            );
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle(&app, cx).await;
            crate::path_filter::filters_land(&app, cx);
            draw(cx);
            app.read_with(cx, |app, _| {
                assert!(app.page == page, "{name} still shows");
                assert!(!status_read_pending(app), "{name}: the read landed");
                assert!(
                    app.working_selected.is_some(),
                    "{name}: the row stays selected"
                );
            });
            assert!(
                entries(&app, cx).is_some_and(|entries| entries.contains(&later)),
                "{name}: Refresh ran"
            );
            assert_eq!(
                focused(cx),
                opened,
                "{name}: focus stays where the page opened it"
            );
            // Back to Changes, focus returns to the list.
            cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
            draw(cx);
            assert!(
                app.read_with(cx, |app, _| app.page == AppPage::Repository
                    && app.mode == WorkspaceMode::Working),
                "{name}: Back shows Changes"
            );
            assert!(on_list(cx), "{name}: Back returns focus to the list");
        }
    }

    /// A write that ends while Settings shows reads the working state, which
    /// leaves focus on Settings, as a read Settings runs does
    /// (`a_status_read_landing_behind_settings_or_projects_leaves_focus_there`):
    /// Stage all pressed with a row selected, and Settings opened while it
    /// ran, once lost Settings' focus to the hidden list as the write's read
    /// landed.
    #[gpui::test]
    async fn a_write_ending_behind_settings_leaves_focus_there(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, _) = checked_out(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let on_list = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).file_focus.is_focused(window))
        };
        select_unstaged(&app, "draft.txt", cx).await;
        assert!(on_list(cx), "the selected row's list holds focus");
        // Stage all held open, as a slow disk holds it, and Settings.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        let opened = focused(cx);
        assert!(opened.is_some() && !on_list(cx), "Settings has focus");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.page == AppPage::Settings, "Settings still shows");
            assert_eq!(app.operation_busy, None, "the write ended");
            assert!(app.working_selected.is_some(), "the row stays selected");
        });
        assert!(
            entry(&app, "draft.txt", cx).staged.is_some(),
            "the write's read landed"
        );
        assert_eq!(focused(cx), opened, "focus stays on Settings");
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        draw(cx);
        assert!(on_list(cx), "Back returns focus to the list");
    }

    /// Changes' Refresh pressed with attribution open on the selected row,
    /// and Settings opened before its read lands: the read closes the
    /// attribution, which leaves focus on Settings, and Back returns focus
    /// to the list the attribution was opened from. Closing it once
    /// returned focus to that list at once, behind Settings; kept for Back,
    /// the attribution's own focus, which Settings opened from, is no
    /// longer drawn there.
    #[gpui::test]
    async fn an_attribution_a_read_closes_behind_settings_returns_focus_on_back(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let on_list = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.read(cx).file_focus.is_focused(window))
        };
        select_unstaged(&app, "draft.txt", cx).await;
        assert!(on_list(cx), "the selected row's list holds focus");
        let row = app.read_with(cx, |app, _| app.working_selected.unwrap().0);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(row, window, cx)));
        crate::blame::lands(&app, cx);
        assert!(app.read_with(cx, |app, _| app.blame.is_visible()));
        assert!(!on_list(cx), "the attribution holds focus");
        // Changes' Refresh, its read held as a slow disk holds it, then
        // Settings.
        let held = app.update(cx, |app, _| app.operations.hold());
        std::fs::write(fixture.path().join("narrow").join("later.txt"), "later\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        assert!(app.read_with(cx, |app, _| app.status_task.is_some()));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        let opened = focused(cx);
        assert!(opened.is_some() && !on_list(cx), "Settings has focus");
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.page == AppPage::Settings, "Settings still shows");
            assert!(app.working_selected.is_some(), "the row stays selected");
            assert!(!app.blame.is_visible(), "the read closes the attribution");
        });
        assert!(
            entries(&app, cx).is_some_and(|entries| entries.contains(&"later.txt".to_owned())),
            "Refresh's read landed"
        );
        assert_eq!(focused(cx), opened, "focus stays on Settings");
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Repository
                && app.mode == WorkspaceMode::Working),
            "Back shows Changes"
        );
        assert!(on_list(cx), "Back returns focus to the list");
    }

    /// History's Refresh opens the repository again, which drops the working
    /// read Changes asked for while a review was prepared: the snapshot it
    /// reads reads the working state itself, as the wait for the review's
    /// dropped read, which Refresh holds, ends. Refresh pressed during
    /// Discard…'s review, then History's Refresh, once left the read noted,
    /// and a later preparation's end, before that snapshot landed or where
    /// it failed, read the working state unasked.
    #[gpui::test]
    async fn historys_refresh_drops_the_read_a_cancelled_preparation_held_back(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        // Discard…'s review, held as a slow disk holds it, and Refresh.
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        std::fs::write(fixture.path().join("narrow").join("later.txt"), "later\n").unwrap();
        cx.simulate_keystrokes(&primary("r"));
        draw(cx);
        assert!(app.read_with(cx, |app, _| status_read_pending(app)));
        cx.simulate_keystrokes(&primary("1"));
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History, "History shows");
            assert!(app.operation_busy.is_some(), "the review still runs");
        });
        // History's Refresh, read here before its snapshot can land.
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        app.read_with(cx, |app, _| {
            assert_eq!(
                app.operation_busy,
                Some("Reviewing file changes…"),
                "Refresh holds the wait for the review's read"
            );
            assert_eq!(
                app.working_read_deferred, None,
                "and drops the read Changes asked for"
            );
        });
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History);
            assert_eq!(app.operation_busy, None);
            assert!(!status_read_pending(app), "no read stays noted");
        });
        assert!(
            entries(&app, cx).is_some_and(|entries| entries.contains(&"later.txt".to_owned())),
            "the snapshot's own read reads the working state"
        );
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "the cancelled review opens nothing"
        );
    }

    /// Each review's preparation, and the series review's remote check, holds
    /// the operation executor, so the working-state read asked for while it
    /// runs waits for it, and its end runs that read, as does the Cancel of
    /// a form whose preparation then never ends (`read_deferred_working`).
    /// The read is asked for here as History's Refresh asks once its
    /// snapshot lands (`open`), which a form over the window leaves
    /// possible. Only Discard…'s end was tested to run it; without its call
    /// an end left the read noted and the rows as they were read last.
    #[gpui::test]
    async fn every_preparations_end_runs_the_read_asked_for_meanwhile(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        use gpui_kit::component::dialog::{Cancel, Confirm};
        type Step = fn(&Entity<GitTurtle>, &mut VisualTestContext);
        /// Lets the dialog that just opened finish its slide-in. The kit's
        /// dialog slides in on GPUI's animation element, which reads the wall
        /// clock rather than the test clock, so no count of drawn frames can
        /// tell the slide-in is over: on a loaded host a frame drawn between
        /// the read of a control's bounds and the press, as the redraw after
        /// the press goes down, moved the control and the press missed it.
        /// Real time passes the slide-in's duration once; only that lower
        /// bound matters, so a slow host changes nothing.
        fn settle_dialog(cx: &mut VisualTestContext) {
            // Drawn first, so the slide-in has begun before the wait.
            crate::views::tests::draw(cx);
            std::thread::sleep(
                *gpui_kit::component::dialog::ANIMATION_DURATION
                    + std::time::Duration::from_millis(50),
            );
            // The frame that finds the slide-in over draws the dialog at
            // rest and asks for no other; a running preparation may keep
            // asking for frames of its own, so the draws are bounded.
            for _ in 0..4 {
                let asked = cx.update(|window, cx| {
                    let asked = window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                    asked
                });
                cx.executor().run_until_parked();
                if asked == 0 {
                    break;
                }
            }
        }
        /// Presses the control `selector` names in the dialog over the
        /// window, once that dialog is at rest.
        fn click(cx: &mut VisualTestContext, selector: &'static str) {
            settle_dialog(cx);
            crate::views::tests::draw(cx);
            let bounds = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            cx.simulate_click(bounds.center(), Modifiers::default());
        }
        /// Create tag's Review tag.
        fn review_tag(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    crate::tags::open_tag_annotation(app, window, cx);
                })
            });
            click(cx, "review-tag");
        }
        /// Ignore…, for the untracked draft, which reviews the file first.
        fn ignore(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
            let draft = entry(app, "draft.txt", cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_ignore(draft, window, cx)));
        }
        /// Ignore's form, opened once Ignore…'s review of the file lands.
        fn ignore_form(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
            ignore(app, cx);
            crate::ignore::tests::ignore_lands(app, cx);
        }
        /// The open Ignore form's Preview rule.
        fn preview_rule(_: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
            cx.update(|window, cx| {
                window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
            });
        }
        /// Edit local commits' Load commits.
        fn load_commits(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.open_interactive_rebase(None, window, cx))
            });
            click(cx, "load-rebase-commits");
        }
        fn cancel(_: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
            cx.update(|window, cx| window.dispatch_action(Box::new(Cancel), cx));
        }
        let (fixture, app, cx, _) = checked_out(cx).await;
        let root = fixture.path().join("narrow");
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        // What opens the form it starts from, if anything, what starts each,
        // with the executor held as a slow disk or remote holds it, and what
        // ends it before it answers, if anything.
        let cases: [(&str, Option<Step>, Step, Option<Step>); 10] = [
            (
                "Tags",
                None,
                |app, cx| {
                    cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)))
                },
                None,
            ),
            (
                "Create tag's Cancel",
                None,
                review_tag,
                Some(|_, cx| click(cx, "cancel-create-tag")),
            ),
            ("Ignore…'s review of the file", None, ignore, None),
            (
                "Ignore's Preview rule",
                Some(ignore_form),
                preview_rule,
                None,
            ),
            (
                "Ignore's Cancel",
                Some(ignore_form),
                preview_rule,
                Some(cancel),
            ),
            (
                "Amend last commit",
                None,
                |app, cx| {
                    let head = app.read_with(cx, |app, _| app.commits[0].oid.clone());
                    cx.update(|window, cx| {
                        app.update(cx, |app, cx| {
                            crate::recovery::prepare_amend(app, head, window, cx)
                        })
                    })
                },
                None,
            ),
            ("Edit local commits' Load commits", None, load_commits, None),
            (
                "Edit local commits' Cancel",
                None,
                load_commits,
                Some(cancel),
            ),
            (
                "A profile's Apply…",
                None,
                |app, cx| {
                    cx.update(|window, cx| {
                        app.update(cx, |app, cx| {
                            crate::profiles::apply_profile(app, window, cx)
                        })
                    })
                },
                None,
            ),
            (
                "Check publication destination…",
                None,
                |app, cx| {
                    cx.update(|window, cx| {
                        crate::rewrite_review::check_publication(app, window, cx)
                    })
                },
                None,
            ),
        ];
        for (index, (case, before, start, end)) in cases.into_iter().enumerate() {
            let later = format!("later-{index}.txt");
            if let Some(before) = before {
                before(&app, cx);
            }
            let held = app.update(cx, |app, _| app.operations.hold());
            start(&app, cx);
            draw(cx);
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_some()),
                "{case} runs"
            );
            std::fs::write(root.join(&later), "later\n").unwrap();
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            app.read_with(cx, |app, _| {
                assert!(app.working_read_pending(), "{case}: the read is asked for");
                assert!(app.status_task.is_none(), "{case}: the read waits for it");
            });
            if let Some(end) = end {
                end(&app, cx);
            }
            drop(held);
            app.read_with(cx, |app, _| app.operations.drain());
            settle(&app, cx).await;
            crate::path_filter::filters_land(&app, cx);
            app.read_with(cx, |app, _| {
                assert!(app.operation_busy.is_none(), "{case} ended");
                assert_eq!(
                    app.working_read_deferred, None,
                    "{case}: no read stays noted"
                );
            });
            assert!(
                entries(&app, cx).is_some_and(|entries| entries.contains(&later)),
                "{case}: the read asked for ran"
            );
            // What its end opened closes before the next.
            cx.update(|window, cx| {
                for _ in 0..4 {
                    if window.has_active_dialog(cx) {
                        window.close_dialog(cx);
                    }
                }
                assert!(!window.has_active_dialog(cx), "{case}: its dialogs close");
            });
            settle(&app, cx).await;
        }
    }

    /// A tab switch or close cancels Discard…'s review and drops the
    /// working read Changes asked for meanwhile with the tab's other reads:
    /// shown again, a tab reads quietly. The read once stayed noted, and the
    /// end of a later preparation in the tab read the working state unasked.
    #[gpui::test]
    async fn a_tab_switch_or_close_drops_the_read_its_tab_asked_for(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, root, other) = beside_another_tab(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);

        // Refresh asked for while the review runs, and the other tab shown.
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        cx.simulate_keystrokes(&primary("r"));
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.status_task.is_none()));
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_ref(), Some(&root));
            assert!(app.mode == WorkspaceMode::Working);
            assert!(!app.working_read_pending(), "the switch dropped the read");
        });
        // A later review's end opens its confirmation and reads nothing.
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        let reads = app.update(cx, |app, _| app.operations.hold());
        cx.executor().run_until_parked();
        app.read_with(cx, |app, _| {
            assert!(app.operation_busy.is_none(), "the review ended");
            assert!(app.status_task.is_none(), "no read the tab did not ask for");
        });
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        cx.update(|window, cx| window.close_dialog(cx));
        drop(reads);
        settle(&app, cx).await;

        // Refresh asked for while the review runs, and the tab closed.
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        cx.simulate_keystrokes(&primary("r"));
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.working_read_pending()));
        cx.update(|window, cx| app.update(cx, |app, cx| app.close_repository_tab(0, window, cx)));
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_ref(), Some(&other));
            assert_eq!(
                app.working_read_deferred, None,
                "the closed tab's read is dropped"
            );
        });
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            None,
            "nothing stays busy"
        );
    }

    /// A tab switch drops the branch comparison Merge… or Rebase… asked for
    /// and ends its busy state once its read ends, as it drops every review
    /// being prepared (`a_tab_switch_drops_every_preparation_alike`): the
    /// dropped comparison never reached its end, which ended it, so
    /// "Comparing branch tips…" once stayed for good, and Changes, in that
    /// tab or another, said "Reading the working tree…" with no end.
    #[gpui::test]
    async fn a_tab_switch_ends_the_branch_comparison_it_drops(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, _, other) = beside_another_tab(cx).await;
        // Merge…'s comparison, held as a slow disk holds it.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.prepare_integration("main".into(), false, window, cx)
            })
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some("Comparing branch tips…")
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        draw(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some("Finishing another tab's review…"),
            "the next tab waits for the dropped comparison's read, saying where it runs"
        );
        std::fs::write(other.join("later.txt"), "later\n").unwrap();
        cx.simulate_keystrokes(&primary("2"));
        draw(cx);
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_ref(), Some(&other));
            assert!(app.mode == WorkspaceMode::Working);
            assert_eq!(app.operation_busy, None);
            assert_eq!(app.operation_error, None);
        });
        assert_eq!(
            entries(&app, cx),
            Some(vec!["draft.txt".to_owned(), "later.txt".to_owned()]),
            "Changes reads"
        );
        assert!(cx.debug_bounds("working-reading").is_none());
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "no plan opens over the other tab"
        );
    }

    /// A tab switch drops every review being prepared alike
    /// (`end_preparations`, `PageSwitch::Tab`), Tags, Discard…'s and
    /// Ignore…'s reviews, Merge…'s comparison, Amend last commit, a
    /// branch's actions and a paused rebase's Continue: nothing it brings
    /// opens, in the tab shown next or in the tab that asked when it shows
    /// again, while both show its label until its read ends, and the next
    /// tab's Changes notes its read rather than queue it behind that read on
    /// the executor every tab shares. A new tab opened meanwhile drops it
    /// too. A switch once ended only Discard…'s review and Merge…'s
    /// comparison, at once, and let the others carry on, so their dialogs
    /// opened over the tab that asked as it showed again; a cold tab's
    /// switch and a new tab cancelled them all at once.
    #[gpui::test]
    async fn a_tab_switch_drops_every_preparation_alike(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, fixture_repository, settle};
        let (fixture, app, cx, root, other) = beside_another_tab(cx).await;
        let head = app.read_with(cx, |app, _| app.commits[0].oid.clone());
        /// The untracked draft's working entry.
        fn draft(app: &GitTurtle) -> gitturtle_core::StatusEntry {
            app.work_status
                .as_ref()
                .and_then(|status| {
                    status
                        .entries
                        .iter()
                        .find(|entry| entry.path == std::path::Path::new("draft.txt"))
                        .cloned()
                })
                .expect("draft.txt is listed")
        }
        // Each preparation, whether Changes asks for it, and its label;
        // Continue rebase last, as it leaves the repository mid-rebase.
        let preparations: [(&str, bool, &str, Prepare); 7] = [
            ("Tags", false, "Reading tags…", |app, _, window, cx| {
                app.open_tags(window, cx)
            }),
            (
                "Discard…",
                true,
                "Reviewing file changes…",
                |app, _, window, cx| app.open_discard(draft(app), window, cx),
            ),
            (
                "Ignore…",
                true,
                "Preparing ignore rule…",
                |app, _, window, cx| app.open_ignore(draft(app), window, cx),
            ),
            (
                "Merge…",
                false,
                "Comparing branch tips…",
                |app, _, window, cx| app.prepare_integration("main".into(), false, window, cx),
            ),
            (
                "Amend last commit",
                false,
                "Preparing recovery action…",
                |app, head, window, cx| crate::recovery::prepare_amend(app, head, window, cx),
            ),
            (
                "main's branch actions",
                false,
                "Preparing branch action…",
                |app, _, window, cx| app.open_contextual_branch("main".into(), false, window, cx),
            ),
            (
                "Continue rebase",
                false,
                "Reading rebase sequence…",
                |app, _, window, cx| app.prepare_interactive_rebase_continue(window, cx),
            ),
        ];
        let busy = |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.operation_busy);
        for (name, changes, label, start) in preparations {
            if name == "Continue rebase" {
                pause_a_reword(&app, &root, cx).await;
            }
            // Asked for in the first tab, held as a slow disk holds it.
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    if changes {
                        app.show_working(window, cx)
                    } else {
                        app.show_history(window, cx)
                    }
                })
            });
            settle(&app, cx).await;
            crate::path_filter::filters_land(&app, cx);
            let held = app.update(cx, |app, _| app.operations.hold());
            let commit = head.clone();
            cx.update(|window, cx| app.update(cx, |app, cx| start(app, commit, window, cx)));
            assert_eq!(busy(cx), Some(label), "{name} is prepared");

            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.path.clone()),
                Some(other.clone())
            );
            assert_eq!(
                busy(cx),
                Some("Finishing another tab's review…"),
                "{name}: the next tab waits for it, saying where it runs"
            );
            // Its working state asked for, as its Changes asks.
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            draw(cx);
            assert!(
                app.read_with(cx, |app, _| app.status_task.is_none()),
                "{name}: no read queues behind it"
            );
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
            });
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.path.clone()),
                Some(root.clone())
            );
            assert_eq!(busy(cx), Some(label), "{name}: so does the tab that asked");

            drop(held);
            preparation_ends(&app, cx).await;
            quiet_reads_land(&app, cx);
            assert!(!dialog_open(cx), "{name}: nothing it brings opens");
            app.read_with(cx, |app, _| {
                assert_eq!(app.operation_busy, None, "{name}: it ends with its read");
                assert!(!status_read_pending(app), "{name}: no read stays noted");
            });
            close_dialogs(cx);
        }

        // A new tab opened while Discard…'s review is prepared drops it too.
        let third = fixture_repository(fixture.path().join("third"))
            .path()
            .to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft(app), window, cx)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(third.clone(), window, cx)
            })
        });
        draw(cx);
        assert_eq!(
            busy(cx),
            Some("Finishing another tab's review…"),
            "a new tab waits for the dropped review"
        );
        drop(held);
        preparation_ends(&app, cx).await;
        quiet_reads_land(&app, cx);
        assert!(!dialog_open(cx), "nothing opens over the new tab");
        assert_eq!(busy(cx), None);
    }

    /// Discard…'s review, dropped as History shows again from Changes,
    /// keeps the window busy through a tab switch or a tab close until the
    /// read its wait is for ends: the read still runs on the executor every
    /// tab shares, so the next tab's Changes notes its read for the wait's
    /// end, and Discard… there waits too. The switch's cancel of Discard…
    /// once ended the busy state the wait held, which went on: Discard…
    /// started again in the next tab under the same label, queued behind
    /// the wait's marker, and the marker's end ended that review's busy
    /// state while it still ran, so a Commit was accepted meanwhile and the
    /// review's confirmation opened as it ran.
    #[gpui::test]
    async fn a_dropped_reviews_wait_keeps_its_busy_state_into_the_next_tab(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        const REVIEWING: &str = "Reviewing file changes…";
        let (_fixture, app, cx, root, other) = beside_another_tab(cx).await;
        for (index, close) in [false, true].into_iter().enumerate() {
            let case = if close { "a tab close" } else { "a tab switch" };
            if index > 0 {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
                });
                settle(&app, cx).await;
                quiet_reads_land(&app, cx);
            }
            assert_eq!(
                app.read_with(cx, |app, _| app.path.clone()),
                Some(root.clone())
            );
            // Discard…'s review in Changes, held as a slow disk holds it,
            // and History shown again, which drops it and waits for it.
            cx.simulate_keystrokes(&primary("2"));
            settle(&app, cx).await;
            crate::path_filter::filters_land(&app, cx);
            let held = app.update(cx, |app, _| app.operations.hold());
            let draft = entry(&app, "draft.txt", cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
            cx.simulate_keystrokes(&primary("1"));
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some(REVIEWING),
                "{case}: History waits for the dropped review"
            );

            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    if close {
                        app.close_repository_tab(0, window, cx)
                    } else {
                        app.switch_repository_tab(1, window, cx)
                    }
                })
            });
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.path.clone()),
                Some(other.clone())
            );
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some(if close {
                    "Finishing a closed tab's review…"
                } else {
                    "Finishing another tab's review…"
                }),
                "{case}: the wait keeps its busy state, saying where it runs"
            );
            // The next tab's Changes notes its read, and its Discard… waits.
            let later = format!("later-{index}.txt");
            std::fs::write(other.join(&later), "later\n").unwrap();
            cx.simulate_keystrokes(&primary("2"));
            draw(cx);
            assert!(
                app.read_with(cx, |app, _| app.status_task.is_none()),
                "{case}: no read queues behind the dropped review"
            );
            let draft = entry(&app, "draft.txt", cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));

            drop(held);
            preparation_ends(&app, cx).await;
            quiet_reads_land(&app, cx);
            assert!(!dialog_open(cx), "{case}: nothing opens");
            app.read_with(cx, |app, _| {
                assert_eq!(app.operation_busy, None, "{case}: the wait ended");
                assert!(!status_read_pending(app), "{case}: no read stays noted");
            });
            assert!(
                entries(&app, cx).is_some_and(|entries| entries.contains(&later)),
                "{case}: the wait's end reads the next tab"
            );

            // Discard… asked for again there holds its own busy state until
            // its review ends, and then its confirmation opens.
            let held = app.update(cx, |app, _| app.operations.hold());
            let draft = entry(&app, "draft.txt", cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
            cx.executor().run_until_parked();
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some(REVIEWING),
                "{case}: the review asked for there"
            );
            assert!(!dialog_open(cx), "{case}: not before its review ends");
            drop(held);
            preparation_ends(&app, cx).await;
            assert!(dialog_open(cx), "{case}: its confirmation opens");
            assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None);
            close_dialogs(cx);
        }
    }

    /// Presses History's Refresh nine times in the tab shown, whose
    /// repository is `root`, while a dropped review's wait runs on `held`,
    /// or a review History asked for is prepared there, which the first
    /// press drops, then lets the read end: each Refresh holds the wait, and its
    /// snapshot notes its working read for the wait's end, so no status
    /// read queues behind the review's read, none is refused as busy and
    /// the rows stay as they were; the wait's end then reads the tab once,
    /// with `later`, written before the first press.
    async fn refreshes_hold_the_wait(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        root: &std::path::Path,
        later: &str,
        held: crate::operations::Held,
        case: &str,
    ) {
        use crate::views::tests::draw;
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_deref(), Some(root), "{case}");
            assert!(app.mode == WorkspaceMode::History, "{case}");
            assert!(
                app.operation_busy.is_some(),
                "{case}: the dropped review's wait"
            );
        });
        let rows = entries(app, cx);
        let reads = app.read_with(cx, |app, _| app.work_generation);
        std::fs::write(root.join(later), "later\n").unwrap();

        for press in 1..=9 {
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
            // Its snapshot lands, which asks for the working state. `settle`
            // would also wait for a status read queued behind the held read.
            let snapshot = app.update(cx, |app, _| app.task.take());
            if let Some(snapshot) = snapshot {
                snapshot.await;
            }
            draw(cx);
            app.read_with(cx, |app, _| {
                assert!(
                    app.operation_busy.is_some(),
                    "{case}: Refresh {press} holds the wait"
                );
                assert_eq!(
                    app.operation_error, None,
                    "{case}: Refresh {press}: no read was refused as busy"
                );
            });
            assert_eq!(
                entries(app, cx),
                rows,
                "{case}: Refresh {press} keeps the rows"
            );
        }
        assert_eq!(
            app.read_with(cx, |app, _| app.work_generation),
            reads,
            "{case}: no status read queues behind the dropped review"
        );

        drop(held);
        preparation_ends(app, cx).await;
        quiet_reads_land(app, cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_deref(), Some(root), "{case}");
            assert_eq!(
                app.operation_busy, None,
                "{case}: the wait ends with the read"
            );
            assert_eq!(app.operation_error, None, "{case}");
            assert_eq!(
                app.work_generation,
                reads.wrapping_add(1),
                "{case}: the wait's end reads the tab once"
            );
            assert!(!status_read_pending(app), "{case}: no read stays noted");
        });
        let landed = entries(app, cx).unwrap_or_default();
        assert!(
            landed.iter().any(|path| path == "draft.txt")
                && landed.iter().any(|path| path == later),
            "{case}: the wait's end reads the tab's working state, {landed:?}"
        );
        assert!(!dialog_open(cx), "{case}: the dropped review opens nothing");
    }

    /// History's Refresh in a tab that did not ask for a dropped review holds
    /// the wait for its read, as the tab switch that dropped it does
    /// (`hold_dropped_preparation`): the read still runs on the executor
    /// every tab shares, so each Refresh's snapshot notes its working read
    /// for the wait's end, which reads once. Ended by the first Refresh, the
    /// wait let each snapshot queue a status read behind that read, each
    /// keeping its slot of the executor's bounded queue though the next
    /// Refresh dropped it, until one was refused as busy and the tab's
    /// Changes lost its rows.
    #[gpui::test]
    async fn historys_refresh_in_another_tab_holds_a_dropped_reviews_wait(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, _, other) = beside_another_tab(cx).await;
        // Discard…'s review in the first tab's Changes, held as a slow disk
        // holds it, dropped as the second tab shows, in History.
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        cx.simulate_keystrokes(&primary("1"));
        draw(cx);
        assert_eq!(entries(&app, cx), Some(vec!["draft.txt".to_owned()]));
        refreshes_hold_the_wait(&app, cx, &other, "later.txt", held, "another tab").await;
    }

    /// History's Refresh holds a dropped review's wait in the tab that asked
    /// for it too, as in any other (`hold_dropped_preparation`), and in a
    /// tab that opens that tab's repository again once it has closed: the
    /// read still runs on the executor every tab shares whoever asked, so
    /// each Refresh's snapshot notes its working read for the wait's end,
    /// which reads once. `open` in place once ended the wait in the tab that
    /// asked, found by its repository's path, as it cancelled that tab's own
    /// preparations, so Discard… started in Changes, History shown again
    /// and Refresh pressed there a few times queued a status read behind the
    /// review's read at each press, until one was refused as busy and
    /// Changes lost its rows. Settings and Projects stay the way out of a
    /// read that never ends.
    #[gpui::test]
    async fn historys_refresh_holds_a_dropped_reviews_wait_in_the_tab_that_asked(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, root) = checked_out(cx).await;
        // Discard…'s review in Changes, held as a slow disk holds it, and
        // History shown again, which drops it and waits for its read.
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        cx.simulate_keystrokes(&primary("1"));
        draw(cx);
        assert_eq!(entries(&app, cx), Some(vec!["draft.txt".to_owned()]));
        refreshes_hold_the_wait(&app, cx, &root, "later.txt", held, "the tab that asked").await;

        // Asked for again, dropped as a second tab shows, and the tab that
        // asked closed there: a tab that opens its repository again matches
        // its path, but never asked.
        let other = another_tab(&fixture, &app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.close_repository_tab(0, window, cx)));
        assert_eq!(app.read_with(cx, |app, _| app.path.clone()), Some(other));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        let snapshot = app.update(cx, |app, _| app.task.take());
        if let Some(snapshot) = snapshot {
            snapshot.await;
        }
        draw(cx);
        refreshes_hold_the_wait(
            &app,
            cx,
            &root,
            "later-again.txt",
            held,
            "a tab that opens the closed tab's repository again",
        )
        .await;
    }

    /// History's Refresh drops a plan History is preparing, Amend last
    /// commit, a branch's actions or a paused rebase's Continue, and waits
    /// for its read as a switch that drops it does
    /// (`wait_for_cancelled_preparation`): the read still runs on the
    /// operation executor, so each Refresh's snapshot notes its working read
    /// for the wait's end, which reads once, and the plan opens nothing.
    /// Refresh once cancelled the plan without a wait, so each press during
    /// a slow read queued a status read behind it, each keeping its slot of
    /// the executor's bounded queue though the next press dropped it, until
    /// one was refused as busy and Changes lost its rows.
    #[gpui::test]
    async fn historys_refresh_waits_for_the_plan_it_drops(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root) = checked_out(cx).await;
        let head = app.read_with(cx, |app, _| app.commits[0].oid.clone());
        for (index, (plan, prepare)) in plans().into_iter().enumerate() {
            if plan == "Continue rebase" {
                pause_a_reword(&app, &root, cx).await;
            }
            // Asked for in History, held as a slow disk holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            let commit = head.clone();
            cx.update(|window, cx| app.update(cx, |app, cx| prepare(app, commit, window, cx)));
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_some()),
                "{plan} is prepared"
            );
            let later = format!("later-{index}.txt");
            refreshes_hold_the_wait(&app, cx, &root, &later, held, plan).await;
        }
    }

    /// A dropped review's wait says where its work runs in a tab that did
    /// not ask for it: the busy label, and the reason the palette and the
    /// Linux Main Menu give for a command it refuses, as Tags… (`busy_reason`),
    /// name another tab's review, or a closed tab's once that tab has closed, in
    /// a tab that opens its repository again too, while the tab that asked
    /// shows the review's own label and the Git operation's reason. The
    /// next tab once read "Reviewing file changes…" as why its own actions
    /// were unavailable, a review it never asked for, and so did a tab that
    /// opened the closed tab's repository again, found by its path.
    #[gpui::test]
    async fn a_dropped_reviews_wait_says_where_it_runs(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, BUSY_REASON, refusals};
        let (_fixture, app, cx, root, other) = beside_another_tab(cx).await;
        // Wide enough that the busy look's progress label takes none of the
        // header's room from Projects, which Changes shows.
        cx.simulate_resize(size(px(1600.), px(900.)));
        let changes = |cx: &mut VisualTestContext| {
            cx.simulate_keystrokes(&primary("2"));
            draw(cx);
        };
        // The busy label drawn, the reason, and the palette's reason for
        // Tags…, once the busy look shows. Projects, which ends the wait,
        // gives none (`projects_refusal`).
        let shown = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            assert!(cx.debug_bounds("operation-progress").is_some());
            assert_eq!(
                refusals::tooltip("projects").as_deref(),
                Some(projects_tooltip(None).as_str())
            );
            let (label, reason) =
                app.read_with(cx, |app, _| (app.operation_shown(), app.busy_reason()));
            let tags = cx.update(|window, cx| {
                app.read(cx)
                    .menu_command_reason(CommandId::Tags, window, cx)
            });
            (label, reason, tags)
        };
        let says = |label: &'static str, reason: &'static str| (Some(label), reason, Some(reason));

        // Discard…'s review in the first tab, held as a slow disk holds it,
        // dropped as the second tab shows.
        changes(cx);
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        changes(cx);
        assert_eq!(app.read_with(cx, |app, _| app.path.clone()), Some(other));
        assert_eq!(
            shown(cx),
            says(
                "Finishing another tab's review…",
                "Wait for another tab's review to finish"
            ),
            "the next tab"
        );

        // The tab that asked shows its own review.
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        changes(cx);
        assert_eq!(
            shown(cx),
            says("Reviewing file changes…", BUSY_REASON),
            "the tab that asked"
        );

        // Closed from the next tab, it is a closed tab's review.
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        changes(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.close_repository_tab(0, window, cx)));
        draw(cx);
        assert_eq!(
            shown(cx),
            says(
                "Finishing a closed tab's review…",
                "Wait for a closed tab's review to finish"
            ),
            "once the tab that asked has closed"
        );

        // A tab that opens the closed tab's repository again never asked
        // for the review either.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        let snapshot = app.update(cx, |app, _| app.task.take());
        if let Some(snapshot) = snapshot {
            snapshot.await;
        }
        changes(cx);
        assert_eq!(app.read_with(cx, |app, _| app.path.clone()), Some(root));
        assert_eq!(
            shown(cx),
            says(
                "Finishing a closed tab's review…",
                "Wait for a closed tab's review to finish"
            ),
            "a tab that opens the closed tab's repository again"
        );

        drop(held);
        preparation_ends(&app, cx).await;
        quiet_reads_land(&app, cx);
        draw(cx);
        assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None);
        assert_eq!(
            cx.update(|window, cx| app
                .read(cx)
                .menu_command_reason(CommandId::Tags, window, cx)),
            None,
            "the reason ends with the wait"
        );
    }

    /// Settings and Projects, opened from a tab that did not ask for a
    /// dropped review, end its wait there as in the tab that asked, by the
    /// label it shows there, "Finishing another tab's review…"
    /// (`DroppedPreparation::holds`): they stay the way out of a read that
    /// never ends, from any tab, and its end, which never runs, opens and
    /// reads nothing once the read ends. Ended only under the review's own
    /// label, the wait would keep that tab's busy state, and every action
    /// Projects offers, until the read ended.
    #[gpui::test]
    async fn settings_and_projects_end_another_tabs_dropped_review(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, _, other) = beside_another_tab(cx).await;
        type Leave = fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>);
        let leave: [(&str, AppPage, Leave); 2] = [
            ("Settings", AppPage::Settings, |app, window, cx| {
                app.show_settings(window, cx)
            }),
            ("Projects", AppPage::Projects, |app, window, cx| {
                app.show_projects(window, cx)
            }),
        ];
        for (index, (page, shown, leave)) in leave.into_iter().enumerate() {
            if index > 0 {
                cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
                });
                settle(&app, cx).await;
                quiet_reads_land(&app, cx);
            }
            // Discard…'s review in the first tab, held as a slow disk holds
            // it, dropped as the second tab shows.
            cx.simulate_keystrokes(&primary("2"));
            settle(&app, cx).await;
            crate::path_filter::filters_land(&app, cx);
            let held = app.update(cx, |app, _| app.operations.hold());
            let draft = entry(&app, "draft.txt", cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw(cx);
            app.read_with(cx, |app, _| {
                assert_eq!(app.path.as_ref(), Some(&other), "{page}");
                assert_eq!(
                    app.operation_busy,
                    Some("Finishing another tab's review…"),
                    "{page}: the second tab waits for the review"
                );
            });

            cx.update(|window, cx| app.update(cx, |app, cx| leave(app, window, cx)));
            draw(cx);
            app.read_with(cx, |app, _| {
                assert!(app.page == shown, "{page} opens");
                assert_eq!(app.operation_busy, None, "{page}: the label clears");
                assert!(
                    app.dropped_preparation.is_none(),
                    "{page}: the wait ends with it"
                );
            });

            // The read ends; off the repository page no quiet read starts.
            drop(held);
            preparation_ends(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert!(app.page == shown, "{page} still shows");
                assert_eq!(app.operation_busy, None, "{page}");
                assert!(!status_read_pending(app), "{page}: no read stays noted");
            });
            assert!(!dialog_open(cx), "{page}: the dropped review opens nothing");
        }
    }

    /// While the window waits only for a dropped review's read, Projects
    /// stays available everywhere it is offered, as its shortcut already
    /// was: the header's Projects, History's Back to Projects, File ▸
    /// Projects…, the palette and the Linux Main Menu, in the tab that asked
    /// and in another, each announced and drawn available with no reason in
    /// its tooltip, and the header's Projects opens Projects and ends the
    /// wait (`projects_refusal`). Projects is the way out of a read that
    /// never ends, so it is never refused for one; it once looked and was
    /// announced unavailable once the wait looked busy, its tooltip giving
    /// the wait's reason, and a click did nothing. A write in progress
    /// still refuses it everywhere, with the write's reason.
    #[gpui::test]
    async fn projects_stays_offered_through_a_dropped_reviews_wait(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, BUSY_REASON, refusals};
        let (_fixture, app, cx, _, other) = beside_another_tab(cx).await;
        // Wide enough that the busy look's progress label takes none of the
        // header's room from Projects, which Changes shows.
        cx.simulate_resize(size(px(1600.), px(900.)));
        let back = format!("Back to Projects · {}[", primary_label());
        let open = projects_tooltip(None);
        // What each place that offers Projects says once the busy look
        // shows, in the mode `keys` opens: `control`, the header's Projects
        // in Changes or History's Back to Projects, as announced and drawn
        // and in its tooltip, then the palette's reason, which the Linux
        // Main Menu gives too, and whether the File menu refuses it.
        let offered = |keys: &str, control: &'static str, cx: &mut VisualTestContext| {
            cx.simulate_keystrokes(&primary(keys));
            draw(cx);
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            (
                refusals::get(control).map(|refusal| (refusal.announced, refusal.dimmed)),
                refusals::tooltip(control).map(|tooltip| tooltip.to_string()),
                cx.update(|window, cx| {
                    app.read(cx)
                        .menu_command_reason(CommandId::Projects, window, cx)
                }),
                file_menu_refuses(cx),
            )
        };
        let available = |tooltip: &str| {
            (
                Some((false, false)),
                Some(tooltip.to_owned()),
                None::<&'static str>,
                false,
            )
        };
        let refused = |tooltip: String, reason: &'static str| {
            (Some((true, true)), Some(tooltip), Some(reason), true)
        };

        // Discard…'s review in the first tab, held as a slow disk holds it,
        // and History shown again, which drops it and waits for its read.
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        cx.simulate_keystrokes(&primary("1"));
        draw(cx);
        for (keys, control, tooltip) in [
            ("2", "projects", open.as_str()),
            ("1", "page-back", back.as_str()),
        ] {
            assert_eq!(
                offered(keys, control, cx),
                available(tooltip),
                "{control} in the tab that asked"
            );
        }
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some("Reviewing file changes…"),
            "under the review's own label"
        );

        // The second tab says it finishes another tab's review.
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        draw(cx);
        assert_eq!(app.read_with(cx, |app, _| app.path.clone()), Some(other));
        for (keys, control, tooltip) in [
            ("1", "page-back", back.as_str()),
            ("2", "projects", open.as_str()),
        ] {
            assert_eq!(
                offered(keys, control, cx),
                available(tooltip),
                "{control} in another tab"
            );
        }
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some("Finishing another tab's review…")
        );

        // The header's Projects opens Projects and ends the wait.
        let projects = cx.debug_bounds("projects").expect("the header's Projects");
        cx.simulate_click(projects.center(), Modifiers::default());
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.page == AppPage::Projects, "Projects opens");
            assert_eq!(app.operation_busy, None, "the label clears");
            assert!(app.dropped_preparation.is_none(), "the wait ends");
        });
        drop(held);
        preparation_ends(&app, cx).await;
        assert!(!dialog_open(cx), "the dropped review opens nothing");

        // A write held open, as a Fetch from an unreachable remote stays,
        // refuses Projects everywhere with its reason.
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        assert_eq!(
            offered("2", "projects", cx),
            refused(projects_tooltip(Some(BUSY_REASON)), BUSY_REASON),
            "the header's Projects during a write"
        );
        let projects = cx.debug_bounds("projects").expect("the header's Projects");
        cx.simulate_click(projects.center(), Modifiers::default());
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.page == AppPage::Repository),
            "Projects waits for the write"
        );
        assert_eq!(
            offered("1", "page-back", cx),
            refused(
                format!("Back to Projects · {BUSY_REASON} · {}[", primary_label()),
                BUSY_REASON
            ),
            "History's Back to Projects during a write"
        );
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        settle(&app, cx).await;
    }

    /// While a review is being prepared, not yet dropped, Projects stays
    /// available wherever it is offered, as its shortcut always was: the
    /// header's Projects over Discard…'s review in Changes and History's
    /// Back to Projects over Tags' read, each announced and drawn
    /// available with no reason in its tooltip once the review looks busy,
    /// the palette and the File menu offering it too, and pressing it opens
    /// Projects and ends the review as the shortcut does
    /// (`projects_ends_busy`). Both once looked and were announced
    /// unavailable, giving the review's label as the reason, while the
    /// shortcut opened Projects and cancelled the review.
    #[gpui::test]
    async fn projects_ends_a_review_being_prepared_wherever_it_is_offered(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::views::tests::{draw, settle};
        use crate::workspace::{BUSY_LOOK_DELAY, refusals};
        let (_fixture, app, cx, _) = checked_out(cx).await;
        cx.simulate_resize(size(px(1600.), px(900.)));
        // What `control` says once the review looks busy: as announced and
        // drawn and in its tooltip, then the palette's reason and whether
        // the File menu refuses Projects.
        let offered = |control: &'static str, cx: &mut VisualTestContext| {
            draw(cx);
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            assert!(
                app.read_with(cx, |app, _| app.busy_shown()),
                "{control}: the review looks busy"
            );
            (
                refusals::get(control).map(|refusal| (refusal.announced, refusal.dimmed)),
                refusals::tooltip(control).map(|tooltip| tooltip.to_string()),
                cx.update(|window, cx| {
                    app.read(cx)
                        .menu_command_reason(CommandId::Projects, window, cx)
                }),
                file_menu_refuses(cx),
            )
        };
        let available = |tooltip: String| (Some((false, false)), Some(tooltip), None, false);

        // Discard…'s review in Changes, held as a slow disk holds it.
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_busy, Some(crate::discard::PREPARING));
            assert!(app.dropped_preparation.is_none(), "not dropped");
        });
        assert_eq!(
            offered("projects", cx),
            available(projects_tooltip(None)),
            "the header's Projects over Discard…'s review"
        );
        let projects = cx.debug_bounds("projects").expect("the header's Projects");
        cx.simulate_click(projects.center(), Modifiers::default());
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.page == AppPage::Projects, "Projects opens");
            assert_eq!(app.operation_busy, None, "the review ends");
        });
        drop(held);
        preparation_ends(&app, cx).await;
        assert!(!dialog_open(cx), "the review opens nothing");

        // Tags' read in History, held too.
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        cx.simulate_keystrokes(&primary("1"));
        settle(&app, cx).await;
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_busy),
            Some(crate::tags::PREPARING)
        );
        assert_eq!(
            offered("page-back", cx),
            available(format!("Back to Projects · {}[", primary_label())),
            "History's Back to Projects over Tags' read"
        );
        let back = cx.debug_bounds("page-back").expect("Back to Projects");
        cx.simulate_click(back.center(), Modifiers::default());
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.page == AppPage::Projects, "Projects opens");
            assert_eq!(app.operation_busy, None, "the read ends");
        });
        drop(held);
        preparation_ends(&app, cx).await;
        assert!(!dialog_open(cx), "Tags opens nothing");
    }

    /// Projects, Back and Refresh pressed again and again while a read that
    /// never ends holds the operation executor queue one status read for
    /// the tab, whichever route asks: Projects ends the wait for Discard…'s
    /// review, so nothing holds the busy state, and each Refresh then asks
    /// for the working state, which the read already waiting answers
    /// (`refresh_worktree`). Each once queued a read of its own, keeping its
    /// slot of the bounded queue though the next dropped its reply, until
    /// one was refused as busy and Changes lost its rows.
    #[gpui::test]
    async fn projects_back_and_refresh_behind_a_read_that_never_ends_queue_one_read(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, root) = checked_out(cx).await;
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        quiet_reads_land(&app, cx);
        let rows = entries(&app, cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        let draft = entry(&app, "draft.txt", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(draft, window, cx)));
        std::fs::write(root.join("later.txt"), "later\n").unwrap();
        for press in 1..=12 {
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
            draw(cx);
            assert!(
                app.read_with(cx, |app, _| app.page == AppPage::Projects),
                "Projects {press} opens"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
            draw(cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
            draw(cx);
            app.read_with(cx, |app, _| {
                assert_eq!(
                    app.operation_error, None,
                    "Refresh {press}: no read was refused as busy"
                );
            });
            assert_eq!(entries(&app, cx), rows, "Refresh {press} keeps the rows");
        }

        drop(held);
        preparation_ends(&app, cx).await;
        quiet_reads_land(&app, cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            None
        );
        let landed = entries(&app, cx).unwrap_or_default();
        assert!(
            landed.iter().any(|path| path == "later.txt"),
            "the waiting read lands the working state, {landed:?}"
        );
        assert!(!dialog_open(cx), "the review opens nothing");
    }

    /// Whether File ▸ Projects… is disabled.
    fn file_menu_refuses(cx: &mut VisualTestContext) -> bool {
        cx.update(|_, cx| cx.get_menus())
            .unwrap_or_default()
            .into_iter()
            .find(|menu| menu.name.as_ref() == "File")
            .and_then(|file| {
                file.items.into_iter().find_map(|item| match item {
                    gpui::OwnedMenuItem::Action { name, disabled, .. } if name == "Projects…" => {
                        Some(disabled)
                    }
                    _ => None,
                })
            })
            .expect("File ▸ Projects…")
    }

    /// Settings and Projects drop the branch comparison Merge… or Rebase…
    /// asked for, as a tab switch does, and end its busy state with it: Git's
    /// answer opens no confirmation over either page, and nothing stays
    /// "Comparing branch tips…". The answer checked only the repository, so
    /// its alert opened over Settings, and Projects waited for the
    /// comparison as for a write. An answer that reaches another page all
    /// the same, its comparison not dropped, opens nothing there either.
    #[gpui::test]
    async fn settings_and_projects_drop_the_branch_comparison(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        let (_fixture, app, cx, _) = checked_out(cx).await;
        type Leave = fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>);
        let leave: [(&str, AppPage, Leave); 3] = [
            ("Settings", AppPage::Settings, |app, window, cx| {
                app.show_settings(window, cx)
            }),
            ("Projects", AppPage::Projects, |app, window, cx| {
                app.show_projects(window, cx)
            }),
            // A way off the page that drops nothing.
            (
                "Another way off the page",
                AppPage::Settings,
                |app, _, cx| {
                    app.page = AppPage::Settings;
                    cx.notify();
                },
            ),
        ];
        for (index, (page, shown, leave)) in leave.into_iter().enumerate() {
            // Merge…'s comparison, held as a slow disk holds it.
            let held = app.update(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.prepare_integration("main".into(), false, window, cx)
                })
            });
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some("Comparing branch tips…"),
                "{page}"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| leave(app, window, cx)));
            draw(cx);
            assert!(
                app.read_with(cx, |app, _| app.page == shown),
                "{page} shows"
            );
            if index < 2 {
                assert_eq!(
                    app.read_with(cx, |app, _| app.operation_busy),
                    None,
                    "{page} ends the comparison"
                );
            }
            drop(held);
            preparation_ends(&app, cx).await;
            assert!(!dialog_open(cx), "{page}: no confirmation opens over it");
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                None,
                "{page}: nothing stays busy"
            );
            cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
            draw(cx);
            assert!(app.read_with(cx, |app, _| app.page == AppPage::Repository));
        }
    }

    /// Every page switch ends the same reviews being prepared
    /// (`GitTurtle::end_preparations`): Tags, Discard…'s and Ignore…'s
    /// reviews of the file and Merge…'s comparison open nothing over the
    /// view shown next, and the working
    /// read Refresh asked for meanwhile runs. Settings and Projects cancel
    /// them at once and read; Changes opened over History and History shown
    /// again from Changes drop them, the window busy until the dropped read
    /// ends, which then reads, so no read queues behind it. Settings left
    /// Tags to open over it, Projects waited for it as for a write, Changes
    /// let Tags and Merge…'s confirmation open over the working list, and
    /// History, shown from Changes, let Discard…'s open over the history.
    #[gpui::test]
    async fn every_page_switch_ends_the_same_preparations(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        let root = fixture.path().join("narrow");
        type Switch = fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>);
        // Each switch, the mode it leaves and the page and mode it shows.
        let switches: [(&str, WorkspaceMode, AppPage, Option<WorkspaceMode>, Switch); 4] = [
            (
                "Settings",
                WorkspaceMode::Working,
                AppPage::Settings,
                None,
                |app, window, cx| app.show_settings(window, cx),
            ),
            (
                "Projects",
                WorkspaceMode::Working,
                AppPage::Projects,
                None,
                |app, window, cx| app.show_projects(window, cx),
            ),
            (
                "Changes",
                WorkspaceMode::History,
                AppPage::Repository,
                Some(WorkspaceMode::Working),
                |app, window, cx| app.show_working(window, cx),
            ),
            (
                "History",
                WorkspaceMode::Working,
                AppPage::Repository,
                Some(WorkspaceMode::History),
                |app, window, cx| app.show_history(window, cx),
            ),
        ];
        /// The untracked draft's working entry.
        fn draft(app: &GitTurtle) -> gitturtle_core::StatusEntry {
            app.work_status
                .as_ref()
                .and_then(|status| {
                    status
                        .entries
                        .iter()
                        .find(|entry| entry.path == std::path::Path::new("draft.txt"))
                        .cloned()
                })
                .expect("draft.txt is listed")
        }
        let preparations: [(&str, Switch); 4] = [
            ("Tags", |app, window, cx| app.open_tags(window, cx)),
            ("Discard…", |app, window, cx| {
                app.open_discard(draft(app), window, cx)
            }),
            ("Ignore…", |app, window, cx| {
                app.open_ignore(draft(app), window, cx)
            }),
            ("Merge…", |app, window, cx| {
                app.prepare_integration("main".into(), false, window, cx)
            }),
        ];
        let mut index = 0;
        for (switch, from, page, mode, leave) in switches {
            for (preparation, start) in preparations {
                index += 1;
                let case = format!("{preparation} then {switch}");
                // Shown where it is asked for, its preparation held as a
                // slow disk holds it, and Refresh asked for meanwhile.
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        if app.page != AppPage::Repository {
                            app.return_from_page(window, cx);
                        }
                        if from == WorkspaceMode::Working {
                            app.show_working(window, cx)
                        } else {
                            app.show_history(window, cx)
                        }
                    })
                });
                settle(&app, cx).await;
                crate::path_filter::filters_land(&app, cx);
                let held = app.update(cx, |app, _| app.operations.hold());
                cx.update(|window, cx| app.update(cx, |app, cx| start(app, window, cx)));
                assert!(
                    app.read_with(cx, |app, _| app.operation_busy.is_some()),
                    "{case}: it is prepared"
                );
                let later = format!("later-{index}.txt");
                std::fs::write(root.join(&later), "later\n").unwrap();
                cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
                assert!(app.read_with(cx, |app, _| app.working_read_deferred.is_some()));

                cx.update(|window, cx| app.update(cx, |app, cx| leave(app, window, cx)));
                draw(cx);
                app.read_with(cx, |app, _| {
                    assert!(app.page == page, "{case}: {switch} shows");
                    if let Some(mode) = mode {
                        assert!(app.mode == mode, "{case}: {switch} shows");
                        assert!(
                            app.operation_busy.is_some(),
                            "{case}: busy until the dropped read ends"
                        );
                        assert!(
                            app.status_task.is_none(),
                            "{case}: no read queues behind it"
                        );
                    } else {
                        assert_eq!(app.operation_busy, None, "{case}: cancelled at once");
                        assert_eq!(
                            app.working_read_deferred, None,
                            "{case}: the read Refresh asked for is taken up"
                        );
                    }
                });
                drop(held);
                preparation_ends(&app, cx).await;
                assert!(!dialog_open(cx), "{case}: nothing opens over it");
                app.read_with(cx, |app, _| {
                    assert_eq!(app.operation_busy, None, "{case}");
                    assert!(!status_read_pending(app), "{case}: no read stays noted");
                });
                assert!(
                    entries(&app, cx).is_some_and(|entries| entries.contains(&later)),
                    "{case}: Refresh ran"
                );
                close_dialogs(cx);
            }
        }
    }

    /// A quiet refresh refused while a write or a review's preparation ran
    /// runs as it ends. Restored in History during a Fetch in the first tab,
    /// the second tab's rescan waited for the Fetch, and with no watch to
    /// register behind it, as when the host's inotify instances have run
    /// out, the tab kept its history and rows as they were after the Fetch
    /// ended, until something else read them. A change reported during a
    /// review's preparation in the shown tab waited the same way.
    #[gpui::test]
    async fn a_quiet_refresh_held_back_by_a_write_runs_as_it_ends(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, fixture_git, settle};
        let (_fixture, app, cx, _, other) = beside_another_tab(cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.watch_through(crate::local_refresh::Backend::Unavailable, window, cx)
            })
        });
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        // A write in the first tab held open, as a Fetch from an unreachable
        // remote stays, and the other tab shown meanwhile, its reads landed.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::StageAll,
                    "Staging files…",
                    window,
                    cx,
                )
            })
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_ref(), Some(&other));
            assert!(app.mode == WorkspaceMode::History);
            assert!(app.operation_busy.is_some());
            assert_eq!(app.commits[0].subject, "Notes");
        });
        // A commit made in a terminal meanwhile, and a new file.
        std::fs::write(other.join("later.txt"), "later\n").unwrap();
        fixture_git(&other, &["add", "later.txt"]);
        fixture_git(&other, &["commit", "--quiet", "-m", "Later"]);
        std::fs::write(other.join("loose.txt"), "loose\n").unwrap();
        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        draw(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_ref(), Some(&other));
            assert_eq!(app.operation_busy, None);
            assert_eq!(app.commits[0].subject, "Later", "History caught up");
        });
        assert_eq!(
            entries(&app, cx),
            Some(vec!["draft.txt".to_owned(), "loose.txt".to_owned()]),
            "and its working state"
        );

        // A change reported while main's branch actions are prepared.
        let held = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_contextual_branch("main".into(), false, window, cx)
            })
        });
        assert!(app.read_with(cx, |app, _| app.operation_busy.is_some()));
        fixture_git(
            &other,
            &["commit", "--quiet", "--allow-empty", "-m", "Again"],
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.queue_automatic_refresh(
                    crate::local_refresh::LocalChange {
                        git: true,
                        worktree: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                )
            })
        });
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_busy, None);
            assert_eq!(app.commits[0].subject, "Again", "History caught up");
        });
        cx.update(|window, cx| window.close_dialog(cx));
        settle(&app, cx).await;
    }

    /// Opens a bare dialog over the page with focus on `opener` and the
    /// window naming `pane`, after a frame that draws them, so the window
    /// records the opener as it records any focused control
    /// (`update_modal_focus`).
    fn open_probe_dialog(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        opener: &FocusHandle,
        pane: Pane,
    ) {
        use crate::views::tests::draw;
        cx.update(|window, cx| {
            app.update(cx, |app, _| app.pane = pane);
            window.focus(opener, cx);
        });
        draw(cx);
        cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog.title("Probe")));
        draw(cx);
        assert!(dialog_open(cx), "the dialog opens");
    }

    /// Closes the dialog as Escape or its Close button does, then lets the
    /// window's repair run in the frame after the close, and the time the
    /// toolkit's close animation runs pass.
    fn close_probe_dialog(cx: &mut VisualTestContext) {
        use crate::views::tests::draw;
        cx.update(|window, cx| window.close_dialog(cx));
        draw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert!(!dialog_open(cx), "the dialog closed");
    }

    fn drawn(app: &Entity<GitTurtle>, focus: &FocusHandle, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| app.read(cx).app_focus.contains(focus, window))
    }

    /// A modal that closes once the control it returns to is gone gives
    /// focus to the shown mode's own list, and the window names that list's
    /// pane, as a tab's restore names it (`return_tab_focus`): History's
    /// commit list is `Pane::History`, the changed-file list of Compare or
    /// Changes `Pane::Files`. The pane once stayed the one named as the modal
    /// opened, so a dialog opened from History's changed files whose commit
    /// left History meanwhile closed onto the commit list with the window
    /// still naming the file list, which a File history or Blame opened
    /// next keeps and restores. The GitHub panel's own close returns focus
    /// before the next frame (`modal_return`), and names the pane as it does.
    #[gpui::test]
    async fn a_modal_closing_onto_the_shown_list_names_its_pane(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, _) = checked_out(cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let pane = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.pane)
        };
        let (commits, files) = app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History && app.selected_commit.is_some());
            assert!(!app.files.is_empty(), "the commit's changed files");
            (app.focus.clone(), app.file_focus.clone())
        });

        // History: opened from the changed-file list, whose commit leaves
        // History while the dialog shows, as a refresh after a reset made
        // elsewhere takes it away; the inspector then draws no file list.
        open_probe_dialog(&app, cx, &files, Pane::Files);
        assert_eq!(
            app.read_with(cx, |app, _| app.modal_return_focus.clone()),
            Some(files.clone()),
            "the window recorded the file list as the dialog opened"
        );
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.selected_commit = None;
                app.selected_file = None;
                cx.notify();
            })
        });
        draw(cx);
        assert!(!drawn(&app, &files, cx), "History draws no file list");
        close_probe_dialog(cx);
        assert_eq!(focused(cx), Some(commits.clone()), "History's commit list");
        assert!(
            pane(&app, cx) == Pane::History,
            "the window names History's pane"
        );

        // Compare: the window still names History's pane, and the dialog
        // returns to History's commit list, which it recorded as its opener
        // as the GitHub panel records it, and which Compare does not draw.
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Compare));
        assert!(!drawn(&app, &commits, cx), "Compare draws no commit list");
        open_probe_dialog(&app, cx, &commits, Pane::History);
        app.update(cx, |app, _| {
            app.modal_return_focus = Some(app.focus.clone())
        });
        close_probe_dialog(cx);
        assert_eq!(focused(cx), Some(files.clone()), "Compare's file list");
        assert!(
            pane(&app, cx) == Pane::Files,
            "the window names the file pane"
        );

        // Changes, closed as the GitHub panel closes, which returns focus
        // itself before the next frame's repair.
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working));
        open_probe_dialog(&app, cx, &files, Pane::History);
        app.update(cx, |app, _| {
            app.modal_return_focus = Some(app.focus.clone())
        });
        cx.update(|window, cx| {
            window.close_dialog(cx);
            let back = app.update(cx, |app, _| app.modal_return(window));
            window.focus(&back, cx);
        });
        assert_eq!(focused(cx), Some(files.clone()), "Changes' file list");
        assert!(pane(&app, cx) == Pane::Files, "named before any frame");
        draw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert_eq!(focused(cx), Some(files.clone()), "the repair agrees");
        assert!(pane(&app, cx) == Pane::Files);
    }

    /// A modal that closes while the control it returns to is still drawn
    /// gives focus back to it and leaves the pane the window named: a dialog
    /// opened from History's changed files closes onto that list, still
    /// naming the file list's pane, rather than taking the commit list's.
    #[gpui::test]
    async fn a_modal_closing_onto_its_drawn_opener_keeps_the_pane(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = checked_out(cx).await;
        let files = app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History && !app.files.is_empty());
            app.file_focus.clone()
        });
        open_probe_dialog(&app, cx, &files, Pane::Files);
        // Focus leaves the opener for a detached control as the dialog
        // closes, so the window's repair runs and decides where it returns.
        let detached = cx.update(|_, cx| cx.focus_handle());
        cx.update(|window, cx| {
            window.close_dialog(cx);
            window.focus(&detached, cx);
        });
        crate::views::tests::draw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        crate::views::tests::draw(cx);
        assert!(drawn(&app, &files, cx), "History still draws its file list");
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            Some(files),
            "focus returns to the file list"
        );
        assert!(
            app.read_with(cx, |app, _| app.pane == Pane::Files),
            "the window still names the file pane"
        );
    }

    /// Focus returns once as a dialog closes, and a control the person
    /// focuses after the close keeps it. Confirmed by Enter, a dialog opened
    /// from History's changed files gives focus back to that list as it
    /// closes, and a commit row clicked next keeps focus on the commit list
    /// once the close animation's 250 ms have passed; closed by Escape, a
    /// click on an empty spot of the window, after the frame that repairs a
    /// close or before it, leaves the window's root focused. The toolkit gave
    /// a confirmed dialog's focus back only after those 250 ms, taking it
    /// from the commit list; the window rechecked 300 ms after a close and
    /// took the root's focus for the shown list, as its repair did in the
    /// frame after the close.
    #[gpui::test]
    async fn a_focus_given_after_a_dialog_closes_keeps(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        let (_fixture, app, cx, _) = checked_out(cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let (commits, files, root) = app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History && !app.files.is_empty());
            (
                app.focus.clone(),
                app.file_focus.clone(),
                app.app_focus.clone(),
            )
        });
        let animation = |cx: &mut VisualTestContext| {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(400));
            draw(cx);
        };

        cx.update(|window, cx| {
            app.update(cx, |app, _| app.pane = Pane::Files);
            window.focus(&files, cx);
        });
        draw(cx);
        cx.update(|window, cx| {
            window.open_dialog(cx, |dialog, _, _| {
                dialog.title("Probe").on_ok(|_, _, _| true)
            })
        });
        draw(cx);
        assert!(dialog_open(cx), "the dialog opens");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(!dialog_open(cx), "Enter confirms the dialog");
        assert_eq!(focused(cx), Some(files.clone()), "back on the file list");
        let row = cx.debug_bounds("history-row-1").expect("a commit row");
        cx.simulate_click(row.center(), Modifiers::default());
        draw(cx);
        assert_eq!(focused(cx), Some(commits.clone()), "the click focuses it");
        animation(cx);
        assert_eq!(focused(cx), Some(commits), "the commit list keeps focus");

        open_probe_dialog(&app, cx, &files, Pane::Files);
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(!dialog_open(cx), "Escape closes the dialog");
        assert_eq!(focused(cx), Some(files.clone()), "back on the file list");
        click_an_empty_spot(&app, cx);
        animation(cx);
        assert_eq!(focused(cx), Some(root.clone()), "after the repair, kept");

        open_probe_dialog(&app, cx, &files, Pane::Files);
        cx.simulate_keystrokes("escape");
        // The frame that sees the dialog gone asks for the repair, which
        // runs as the next frame starts; the click comes between.
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.executor().run_until_parked();
        assert!(!dialog_open(cx), "Escape closes the dialog");
        click_an_empty_spot(&app, cx);
        animation(cx);
        assert_eq!(focused(cx), Some(root), "before the repair, kept");
    }

    /// A dialog opened while another closes, before the window's repair
    /// gives the first one's focus back, keeps the first one's return: a
    /// dialog over History's changed files, whose return a restore under it
    /// made the commit list (`return_modal_to`), closes by Escape, and the
    /// command palette, opened by its shortcut in the frame after, closes
    /// by Escape onto the commit list. The first one's return was taken as
    /// it closed, so the palette closed onto the changed files, the control
    /// the toolkit had captured as the first opened.
    #[gpui::test]
    async fn a_dialog_opened_as_another_closes_keeps_its_return(cx: &mut TestAppContext) {
        use crate::views::tests::draw;
        let (_fixture, app, cx, _) = checked_out(cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let (commits, files) = app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History && !app.files.is_empty());
            (app.focus.clone(), app.file_focus.clone())
        });
        open_probe_dialog(&app, cx, &files, Pane::Files);
        app.update(cx, |app, _| app.return_modal_to(Some(commits.clone())));
        cx.simulate_keystrokes("escape");
        // The frame that sees the dialog gone asks for the repair; the
        // palette opens before the next frame runs it.
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.executor().run_until_parked();
        assert!(!dialog_open(cx), "Escape closes the dialog");
        cx.simulate_keystrokes(&primary("shift-p"));
        draw(cx);
        assert!(dialog_open(cx), "the palette opens");
        assert_eq!(
            app.read_with(cx, |app, _| app.modal_return_focus.clone()),
            Some(commits.clone()),
            "the palette keeps the first dialog's return"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert!(!dialog_open(cx), "Escape closes the palette");
        assert_eq!(focused(cx), Some(commits), "the commit list");
    }

    /// Opens Settings with nothing focused and the window naming `pane`,
    /// then goes Back, and lets the frames that give focus back run.
    fn back_from_settings_with_nothing_focused(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        pane: Pane,
    ) {
        use crate::views::tests::draw;
        cx.update(|window, cx| {
            window.blur(cx);
            app.update(cx, |app, cx| {
                app.pane = pane;
                app.show_settings(window, cx);
            })
        });
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(app.page == AppPage::Settings, "Settings shows");
            assert!(app.page_return_focus.is_none(), "nothing was focused");
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.page == AppPage::Repository));
    }

    /// Back from Settings opened with nothing focused puts focus on the
    /// shown mode's own list, and the window names that list's pane, as
    /// choosing a commit or a file names it: History's commit list is
    /// `Pane::History`, Changes' file list `Pane::Files`. Back once focused
    /// the list alone, which then reached the frame-end check
    /// (`return_tab_focus`) as the control to return to, and that check
    /// names a pane only for a control the page no longer draws: from
    /// History with the file list's pane named, focus came back to the
    /// commit list with the window still naming the file list.
    #[gpui::test]
    async fn back_with_nothing_focused_names_the_lists_pane(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, _) = checked_out(cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let pane = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.pane)
        };
        let (commits, files) = app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History);
            (app.focus.clone(), app.file_focus.clone())
        });

        back_from_settings_with_nothing_focused(&app, cx, Pane::Files);
        assert_eq!(focused(cx), Some(commits), "History's commit list");
        assert!(
            pane(&app, cx) == Pane::History,
            "the window names History's pane"
        );

        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working));
        back_from_settings_with_nothing_focused(&app, cx, Pane::History);
        assert_eq!(focused(cx), Some(files), "Changes' file list");
        assert!(
            pane(&app, cx) == Pane::Files,
            "the window names the file pane"
        );
    }

    /// A window whose newest commit changes two files, so Down moves
    /// Compare's file selection, with both edited again in the working
    /// tree, so Changes lists two rows, at 1000 × 680 in History with the
    /// newest commit selected.
    async fn two_files_checked_out(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
        use crate::views::tests::{fixture_git, history_window_from, settle};
        let (fixture, app, cx) = history_window_from(
            cx,
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 5\nfirst\nM 100644 inline notes.txt\ndata 6\nnotes\nM 100644 inline other.txt\ndata 6\nother\n\ncommit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000001 +0000\ndata 6\nsecond\nM 100644 inline notes.txt\ndata 7\nnotes2\nM 100644 inline other.txt\ndata 7\nother2\n\ndone\n",
        )
        .await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = fixture.path().join("narrow");
        fixture_git(&root, &["reset", "--hard", "--quiet"]);
        std::fs::write(root.join("notes.txt"), "edited notes\n").unwrap();
        std::fs::write(root.join("other.txt"), "edited other\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        (fixture, app, cx)
    }

    /// Clicks an empty spot of the window, the status bar's leading
    /// padding, as a person clicks where nothing acts. GPUI gives such a
    /// press to the window's root, which tracks focus, so the root takes
    /// focus and nothing shows it: AT-SPI reads only the window's frame
    /// focused.
    fn click_an_empty_spot(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        let bar = cx.debug_bounds("status-bar").expect("the status bar");
        cx.simulate_click(
            point(bar.left() + px(4.), bar.center().y),
            Modifiers::default(),
        );
        crate::views::tests::draw(cx);
        let root = app.read_with(cx, |app, _| app.app_focus.clone());
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            Some(root),
            "the click leaves the window's root focused"
        );
    }

    /// Asserts focus rests on `list` drawn with its ring, with the window
    /// naming `pane`.
    fn assert_lands_on(
        app: &Entity<GitTurtle>,
        list: &FocusHandle,
        pane: Pane,
        cx: &mut VisualTestContext,
        at: &str,
    ) {
        crate::views::tests::draw(cx);
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            Some(list.clone()),
            "{at}: focus"
        );
        app.read_with(cx, |app, _| {
            assert!(app.pane == pane, "{at}: the window names its pane");
            assert!(app.focus_ring.shows(list), "{at}: its ring");
        });
    }

    /// Opens Settings by its shortcut and leaves it by `back`.
    fn settings_and_back(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        back: impl Fn(&mut VisualTestContext),
    ) {
        use crate::views::tests::draw;
        cx.simulate_keystrokes(&primary(","));
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.page == AppPage::Settings));
        back(cx);
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.page == AppPage::Repository));
    }

    /// After a click on an empty spot of the window, which leaves the
    /// window's root focused, Settings and Back put focus on the shown
    /// mode's own list, drawn with its ring and named as the window's pane,
    /// so Down moves the selection: History's commit list by the Back
    /// shortcut, Compare's changed-file list by Escape, and Changes' file
    /// list by "Back to repository" pressed with the pointer, whose press
    /// ends the ring a key draws. Back gave the root back, which shows no
    /// focus, and Down moved nothing. A control focused as Settings opened
    /// still gets focus back, with its ring.
    #[gpui::test]
    async fn back_after_a_click_on_an_empty_spot_lands_on_the_shown_list(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = two_files_checked_out(cx).await;
        let (commits, files) = app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History && app.selected_commit == Some(0));
            assert_eq!(app.files.len(), 2, "the newest commit's two files");
            (app.focus.clone(), app.file_focus.clone())
        });
        let shortcut = |cx: &mut VisualTestContext| cx.simulate_keystrokes(&primary("["));

        // The control route: History's file list focused, its pane named.
        cx.update(|window, cx| {
            app.update(cx, |app, _| app.pane = Pane::Files);
            window.focus(&files, cx);
        });
        draw(cx);
        settings_and_back(&app, cx, shortcut);
        assert_lands_on(&app, &files, Pane::Files, cx, "the file list it left");

        // History, the file list's pane still named as the click lands.
        click_an_empty_spot(&app, cx);
        settings_and_back(&app, cx, shortcut);
        assert_lands_on(&app, &commits, Pane::History, cx, "History");
        cx.simulate_keystrokes("down");
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.selected_commit),
            Some(1),
            "Down moves the commit selection"
        );

        // Compare, left by Escape.
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Compare));
        click_an_empty_spot(&app, cx);
        settings_and_back(&app, cx, |cx| cx.simulate_keystrokes("escape"));
        assert_lands_on(&app, &files, Pane::Files, cx, "Compare");
        cx.simulate_keystrokes("down");
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.selected_file),
            Some(1),
            "Down moves the file selection"
        );

        // Changes, left by "Back to repository" pressed with the pointer,
        // which ends the ring a key would draw.
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        select_unstaged(&app, "notes.txt", cx).await;
        let first = app.read_with(cx, |app, _| app.working_selected);
        click_an_empty_spot(&app, cx);
        settings_and_back(&app, cx, |cx| {
            // A pointer press, then what the button's click runs.
            click_an_empty_spot(&app, cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        });
        assert_lands_on(&app, &files, Pane::Files, cx, "Changes");
        cx.simulate_keystrokes("down");
        settle(&app, cx).await;
        let moved = app.read_with(cx, |app, _| app.working_selected);
        assert!(
            moved.is_some() && moved != first,
            "Down moves the working selection"
        );
    }

    /// File history and Blame opened after a click on an empty spot of the
    /// window, which leaves the window's root focused, return focus to the
    /// shown mode's own list as they close, drawn with its ring and named
    /// as the window's pane, so Down moves the selection: File history
    /// closed onto History's commit list, Blame onto Changes' file list,
    /// each opened as the palette opens it, from the root it gives focus
    /// back to. Both gave the root back, which shows no focus.
    #[gpui::test]
    async fn file_history_and_blame_closed_after_a_click_on_an_empty_spot_land_on_the_shown_list(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::settle;
        let (_fixture, app, cx) = two_files_checked_out(cx).await;
        let (commits, files) =
            app.read_with(cx, |app, _| (app.focus.clone(), app.file_focus.clone()));

        // File history from History, the file list's pane named.
        app.update(cx, |app, _| app.pane = Pane::Files);
        click_an_empty_spot(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_file_history(0, window, cx)));
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.file_history.is_active()));
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| !app.file_history.is_active()
            && app.mode == WorkspaceMode::History));
        assert_lands_on(&app, &commits, Pane::History, cx, "File history closed");
        cx.simulate_keystrokes("down");
        settle(&app, cx).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.selected_commit),
            Some(1),
            "Down moves the commit selection"
        );

        // Blame from Changes, History's pane named.
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        select_unstaged(&app, "notes.txt", cx).await;
        let first = app.read_with(cx, |app, _| app.working_selected);
        app.update(cx, |app, _| app.pane = Pane::History);
        click_an_empty_spot(&app, cx);
        let row = first.unwrap().0;
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(row, window, cx)));
        crate::blame::lands(&app, cx);
        assert!(app.read_with(cx, |app, _| app.blame.is_visible()));
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| !app.blame.is_visible()
            && app.mode == WorkspaceMode::Working));
        assert_lands_on(&app, &files, Pane::Files, cx, "Blame closed");
        cx.simulate_keystrokes("down");
        settle(&app, cx).await;
        let moved = app.read_with(cx, |app, _| app.working_selected);
        assert!(
            moved.is_some() && moved != first,
            "Down moves the working selection"
        );
    }

    /// Focuses History's commit list, naming its pane, and opens File
    /// history for the selected commit's first file, which takes focus to
    /// its own list, the changed-file list's handle, once its first page and
    /// that revision's preview have landed: a page landing later selects its
    /// revision, focusing that list, wherever the test has moved focus.
    async fn file_history_from_the_commit_list(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) {
        use crate::views::tests::draw;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.pane = Pane::History;
                window.focus(&app.focus, cx);
            })
        });
        draw(cx);
        crate::file_history::tests::open_landed(app, cx).await;
        assert!(app.read_with(cx, |app, _| app.file_history.is_active()));
        assert!(
            cx.update(|window, cx| app.read(cx).file_focus.is_focused(window)),
            "File history's list holds focus"
        );
    }

    /// File history closed behind Settings, as the write Settings' identity
    /// Save starts closes it, leaves focus on Settings, and Back lands on
    /// the control File history was opened from, as File history closed on
    /// the shown page lands there at once. The restore once focused that
    /// control behind Settings, where nothing drew it and the next key
    /// reached it.
    #[gpui::test]
    async fn file_history_closed_behind_settings_leaves_focus_there_until_back(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx) = two_files_checked_out(cx).await;
        let commits = app.read_with(cx, |app, _| app.focus.clone());
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));

        // Closed on the shown page, it lands on the commit list at once.
        file_history_from_the_commit_list(&app, cx).await;
        cx.simulate_keystrokes("escape");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| !app.file_history.is_active()));
        assert_lands_on(&app, &commits, Pane::History, cx, "File history closed");

        // Closed behind Settings by the write its identity Save starts.
        file_history_from_the_commit_list(&app, cx).await;
        cx.simulate_keystrokes(&primary(","));
        draw(cx);
        let opened = focused(cx);
        assert!(app.read_with(cx, |app, _| app.page == AppPage::Settings));
        assert!(
            opened.is_some() && opened != Some(commits.clone()),
            "Settings has focus"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::SetIdentity {
                        name: "GitTurtle QA".into(),
                        email: "qa@example.invalid".into(),
                    },
                    "Saving repository identity…",
                    window,
                    cx,
                )
            })
        });
        assert!(
            app.read_with(cx, |app, _| !app.file_history.is_active()),
            "the write closes File history"
        );
        assert_eq!(focused(cx), opened, "focus stays on Settings");
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.page == AppPage::Settings
            && app.operation_busy.is_none()));
        assert_eq!(focused(cx), opened, "focus stays on Settings as it ends");
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        assert!(app.read_with(cx, |app, _| app.page == AppPage::Repository
            && app.mode == WorkspaceMode::History));
        assert_lands_on(&app, &commits, Pane::History, cx, "Back from Settings");
    }

    /// File history restored while a dialog covers the page leaves focus in
    /// the dialog, and the dialog's close lands on the control File history
    /// was opened from: not on File history's list, which the dialog was
    /// opened from and the toolkit gives focus back to, and which History
    /// still draws as its changed-file list. The restore once focused that
    /// control behind the dialog, where nothing drew it and the next key
    /// reached it.
    #[gpui::test]
    async fn file_history_restored_under_a_dialog_lands_on_its_control_as_it_closes(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = two_files_checked_out(cx).await;
        let (commits, files) =
            app.read_with(cx, |app, _| (app.focus.clone(), app.file_focus.clone()));
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        file_history_from_the_commit_list(&app, cx).await;
        open_probe_dialog(&app, cx, &files, Pane::Files);
        let in_dialog = focused(cx);
        assert!(
            in_dialog.is_some() && in_dialog != Some(files.clone()),
            "the dialog has focus"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(app.close_file_history(window, cx), "File history closes")
            })
        });
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::History));
        assert!(drawn(&app, &files, cx), "History draws its file list");
        assert_eq!(focused(cx), in_dialog, "focus stays in the dialog");
        close_probe_dialog(cx);
        assert_eq!(focused(cx), Some(commits), "the dialog closes onto it");
        assert!(
            app.read_with(cx, |app, _| app.pane == Pane::History),
            "the window names History's pane"
        );
    }

    /// A revision inspection opened from Changes over a row whose preview
    /// had not landed, closed by a write started while a dialog covers the
    /// page, selects the row again for its preview
    /// (`select_restored_working`) and leaves focus in the dialog, whose
    /// close lands on Changes' file list. Selecting the row once focused
    /// that list behind the dialog. The dialog is opened from that list, so
    /// this checks only that the selection moves no focus under a modal;
    /// `a_dialog_opened_in_quick_open_closes_onto_the_list_its_close_selects`
    /// tells the list's retarget apart.
    #[gpui::test]
    async fn an_inspection_a_write_closes_under_a_dialog_leaves_the_rows_focus_to_its_close(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        use gitturtle_core::{HistoryCancellation, PathScope};
        let (_fixture, app, cx, _) = checked_out(cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        select_unstaged(&app, "draft.txt", cx).await;
        let files = app.read_with(cx, |app, _| app.file_focus.clone());
        // The row's preview gone, as one that failed or still reads leaves
        // it, and Quick Open's tracked source shown over it.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.clear_preview();
                let repo = app.repository.clone().expect("a repository");
                let paths = repo
                    .search_tracked_paths(
                        &PathScope::Worktree,
                        "notes",
                        &HistoryCancellation::default(),
                    )
                    .expect("tracked paths");
                let entry = paths.entries.first().expect("the file").clone();
                app.show_tracked_file(paths.scope.clone(), entry, window, cx);
            })
        });
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.revision_inspection.is_active()));
        open_probe_dialog(&app, cx, &files, Pane::Files);
        let in_dialog = focused(cx);
        assert!(
            in_dialog.is_some() && in_dialog != Some(files.clone()),
            "the dialog has focus"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::SetIdentity {
                        name: "GitTurtle QA".into(),
                        email: "qa@example.invalid".into(),
                    },
                    "Saving repository identity…",
                    window,
                    cx,
                )
            })
        });
        app.read_with(cx, |app, _| {
            assert!(!app.revision_inspection.is_active(), "the write closes it");
            assert!(app.mode == WorkspaceMode::Working && app.working_selected.is_some());
        });
        assert_eq!(focused(cx), in_dialog, "focus stays in the dialog");
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        draw(cx);
        assert_eq!(focused(cx), in_dialog, "focus stays as the write ends");
        close_probe_dialog(cx);
        assert_eq!(focused(cx), Some(files), "the dialog closes onto the list");
    }

    /// A status read landing under a dialog opened from a control other
    /// than Changes' file list selects the row again for its preview and
    /// leaves the dialog returning to that control: only a restore, whose
    /// inspection the dialog was opened from, returns it to the list. The
    /// read once focused the list behind the dialog.
    #[gpui::test]
    async fn a_status_read_under_a_dialog_leaves_it_returning_to_its_opener(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx, _) = checked_out(cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        select_unstaged(&app, "draft.txt", cx).await;
        let filter = app.read_with(cx, |app, cx| app.working_filter.read(cx).focus_handle(cx));
        open_probe_dialog(&app, cx, &filter, Pane::Files);
        let in_dialog = focused(cx);
        assert!(
            in_dialog.is_some() && in_dialog != Some(filter.clone()),
            "the dialog has focus"
        );
        std::fs::write(fixture.path().join("narrow").join("later.txt"), "later\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        draw(cx);
        assert!(
            entries(&app, cx).is_some_and(|entries| entries.contains(&"later.txt".to_owned())),
            "the read landed"
        );
        assert!(app.read_with(cx, |app, _| app.working_selected.is_some()));
        assert_eq!(focused(cx), in_dialog, "focus stays in the dialog");
        close_probe_dialog(cx);
        assert_eq!(
            focused(cx),
            Some(filter),
            "the dialog closes onto its opener"
        );
    }

    /// The focus handle of the editor the text pane shows.
    fn shown_editor(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> FocusHandle {
        app.read_with(cx, |app, cx| match app.shown_text() {
            Some(crate::views::ShownText::Editor(editor)) => editor.read(cx).focus_handle(cx),
            _ => panic!("the text pane shows an editor"),
        })
    }

    /// Starts the identity write Settings' Save starts, which closes the
    /// open inspections, and lets it end.
    async fn write_identity(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.write(
                    gitturtle_core::WriteCommand::SetIdentity {
                        name: "GitTurtle QA".into(),
                        email: "qa@example.invalid".into(),
                    },
                    "Saving repository identity…",
                    window,
                    cx,
                )
            })
        });
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(app, cx).await;
    }

    /// A dialog opened from Quick Open's tracked source, whose editor goes
    /// with the inspection, closes onto Changes' file list where a write
    /// closes the inspection under it and Changes selects its row again:
    /// not onto the filter field the inspection was opened from, which the
    /// restore saved, as the list takes focus there on the shown page.
    #[gpui::test]
    async fn a_dialog_opened_in_quick_open_closes_onto_the_list_its_close_selects(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{draw, settle};
        use gitturtle_core::{HistoryCancellation, PathScope};
        let (_fixture, app, cx, _) = checked_out(cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        cx.simulate_keystrokes(&primary("2"));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        select_unstaged(&app, "draft.txt", cx).await;
        let (files, filter) = app.read_with(cx, |app, cx| {
            (
                app.file_focus.clone(),
                app.working_filter.read(cx).focus_handle(cx),
            )
        });
        // The row's preview gone, focus on the filter field, and Quick
        // Open's tracked source shown from there.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.clear_preview();
                window.focus(&filter, cx);
                let repo = app.repository.clone().expect("a repository");
                let paths = repo
                    .search_tracked_paths(
                        &PathScope::Worktree,
                        "notes",
                        &HistoryCancellation::default(),
                    )
                    .expect("tracked paths");
                let entry = paths.entries.first().expect("the file").clone();
                app.show_tracked_file(paths.scope.clone(), entry, window, cx);
            })
        });
        settle(&app, cx).await;
        draw(cx);
        let source = shown_editor(&app, cx);
        open_probe_dialog(&app, cx, &source, Pane::Files);
        let in_dialog = focused(cx);
        write_identity(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(!app.revision_inspection.is_active(), "the write closes it");
            assert!(app.mode == WorkspaceMode::Working && app.working_selected.is_some());
        });
        draw(cx);
        assert!(drawn(&app, &filter, cx), "Changes draws the filter field");
        assert_eq!(focused(cx), in_dialog, "focus stays in the dialog");
        close_probe_dialog(cx);
        assert_eq!(focused(cx), Some(files), "the dialog closes onto the list");
    }

    /// A dialog opened from File history's preview, whose editor goes with
    /// File history, closes onto History's changed-file list that File
    /// history was opened from where a write closes File history under it:
    /// not onto History's commit list, where a closing modal falls back
    /// when the control it was opened from is gone.
    #[gpui::test]
    async fn a_dialog_opened_in_file_history_closes_onto_its_saved_control(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = two_files_checked_out(cx).await;
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let files = app.read_with(cx, |app, _| app.file_focus.clone());
        cx.update(|window, cx| {
            app.update(cx, |app, _| app.pane = Pane::Files);
            window.focus(&files, cx);
        });
        draw(cx);
        // Its first revision's preview shown, read on the window's reader
        // after the page that selects it.
        crate::file_history::tests::open_landed(&app, cx).await;
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.file_history.is_active()));
        let preview = shown_editor(&app, cx);
        open_probe_dialog(&app, cx, &preview, Pane::Files);
        let in_dialog = focused(cx);
        write_identity(&app, cx).await;
        assert!(app.read_with(cx, |app, _| !app.file_history.is_active()
            && app.mode == WorkspaceMode::History));
        draw(cx);
        assert_eq!(focused(cx), in_dialog, "focus stays in the dialog");
        close_probe_dialog(cx);
        assert_eq!(focused(cx), Some(files), "the dialog closes onto the list");
        assert!(
            app.read_with(cx, |app, _| app.pane == Pane::Files),
            "the window names the file pane"
        );
    }

    /// A dialog opened and closed with no frame between, with a restore run
    /// under it, leaves nothing for a later modal: the next frame that shows
    /// no modal forgets the control the restore replaced, which a later
    /// modal would otherwise keep as its own and give way from as it closes.
    #[gpui::test]
    async fn a_frame_without_a_modal_forgets_the_control_a_restore_replaced(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = two_files_checked_out(cx).await;
        file_history_from_the_commit_list(&app, cx).await;
        cx.update(|window, cx| {
            window.open_dialog(cx, |dialog, _, _| dialog.title("Probe"));
            app.update(cx, |app, cx| {
                assert!(app.close_file_history(window, cx), "File history closes");
                assert!(
                    app.modal_replaced_focus.is_some(),
                    "the restore replaced it"
                );
            });
            window.close_dialog(cx);
        });
        draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.modal_replaced_focus.is_none()),
            "the frame forgets it"
        );
    }

    /// A dialog opened after a click on an empty spot of the window, which
    /// leaves the window's root focused, gives focus to the shown mode's own
    /// list as it closes, named as the window's pane, rather than to the
    /// root the toolkit gives it back to, which shows no focus.
    #[gpui::test]
    async fn a_modal_opened_after_a_click_on_an_empty_spot_closes_onto_the_shown_list(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::draw;
        let (_fixture, app, cx) = two_files_checked_out(cx).await;
        let commits = app.read_with(cx, |app, _| app.focus.clone());
        app.update(cx, |app, _| app.pane = Pane::Files);
        click_an_empty_spot(&app, cx);
        cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog.title("Probe")));
        draw(cx);
        assert!(dialog_open(cx), "the dialog opens");
        close_probe_dialog(cx);
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            Some(commits),
            "History's commit list"
        );
        assert!(
            app.read_with(cx, |app, _| app.pane == Pane::History),
            "the window names History's pane"
        );
    }

    #[test]
    fn project_hub_return_requires_a_resolved_repository() {
        assert_eq!(
            return_page(AppPage::Projects, AppPage::Settings, false),
            AppPage::Projects
        );
        assert_eq!(
            return_page(AppPage::Projects, AppPage::Settings, true),
            AppPage::Repository
        );
    }
}
