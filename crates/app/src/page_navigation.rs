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
        // A preparation cancelled above ended its busy state with it.
        if let Some(label) = busy
            && self.operation_busy.is_none()
        {
            self.wait_for_dropped_preparation(label, window, cx);
        }
    }

    /// Keeps the window busy as `label`, the preparation a switch between
    /// History and Changes just dropped, until the operation executor
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
    /// rows. Settings and Projects stay the way out of a read that never
    /// ends ([`Self::end_dropped_preparation`]).
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
            let generation = self.modal_focus_generation;
            let owner = cx.entity().downgrade();
            let immediate_previous = previous.clone();
            window.on_next_frame(move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.restore_detached_modal_focus(
                        generation,
                        immediate_previous.as_ref(),
                        window,
                        cx,
                    );
                });
            });
            // The pinned toolkit restores its captured focus after a 250 ms
            // close animation. Recheck once afterward; never poll or take focus
            // from a new dialog, a surviving editor, or a Root sibling menu.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(300))
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.restore_detached_modal_focus(generation, previous.as_ref(), window, cx);
                });
            })
            .detach();
        } else if !modal_open && self.app_focus.contains_focused(window, cx) {
            self.modal_return_focus = window.focused(cx);
        }
        self.modal_was_open = modal_open;
    }

    fn restore_detached_modal_focus(
        &mut self,
        generation: u64,
        previous: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal_focus_generation != generation
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
            || !window.context_stack().is_empty()
        {
            return;
        }
        self.modal_return_target(previous, window).focus(window, cx);
        window.refresh();
    }

    /// Where focus returns as a modal closes: `previous` while it is still
    /// drawn, else the shown mode's own list, else the window's root.
    fn modal_return_target<'a>(
        &'a self,
        previous: Option<&'a FocusHandle>,
        window: &Window,
    ) -> &'a FocusHandle {
        // In this window every attached control inherits Root's key context,
        // including sibling popup menus outside app_focus. A missing/detached
        // handle resolves to GPUI's synthetic dispatch root with no contexts.
        // Comparing a handle with itself cannot establish attachment.
        let fallback = match (self.page, self.mode) {
            (AppPage::Repository, WorkspaceMode::History) => &self.focus,
            (AppPage::Repository, _) => &self.file_focus,
            _ => &self.app_focus,
        };
        previous
            .filter(|focus| self.app_focus.contains(focus, window))
            .or_else(|| {
                self.app_focus
                    .contains(fallback, window)
                    .then_some(fallback)
            })
            .unwrap_or(&self.app_focus)
    }

    /// Where a modal that set `modal_return_focus` as it opened returns focus
    /// as it closes, so it can do so before the next frame's repair, which
    /// returns it to the same place: no frame leaves focus on the window's
    /// root, where nothing shows it.
    pub(super) fn modal_return(&self, window: &Window) -> FocusHandle {
        self.modal_return_target(self.modal_return_focus.as_ref(), window)
            .clone()
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
        }
        let focus = self.page_return_focus.take().unwrap_or_else(|| {
            if self.mode == WorkspaceMode::History {
                self.focus.clone()
            } else {
                self.file_focus.clone()
            }
        });
        let path = self.path.clone();
        let owner = cx.entity().downgrade();
        // Restore after the retained editor/list is attached to this frame.
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.page == AppPage::Repository && this.path == path {
                    focus.focus(window, cx);
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
        let projects = "Open Projects".to_owned();
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
            Some(format!("{projects} · {BUSY_REASON}")),
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
        let rest = "Open Projects";
        let waiting = format!("{rest} · {BUSY_REASON}");
        let projects = cx.debug_bounds("projects").expect("the header's Projects");
        cx.simulate_mouse_move(projects.center(), None, Modifiers::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(600));
        draw(cx);
        assert!(shows(cx, rest), "at rest");

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
        assert!(shows(cx, rest), "through the busy look's delay");
        cx.executor().advance_clock(BUSY_LOOK_DELAY);
        draw(cx);
        assert_eq!(
            cx.debug_bounds("projects"),
            Some(projects),
            "Projects stays under the pointer"
        );
        assert!(shows(cx, &waiting), "once the write looks busy");
        assert!(!shows(cx, rest));

        drop(held);
        let write = app
            .update(cx, |app, _| app.operation_task.take())
            .expect("the write");
        write.await;
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.operation_busy.is_none()));
        draw(cx);
        assert!(shows(cx, rest), "once the write ends");
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
        fn click(cx: &mut VisualTestContext, selector: &'static str) {
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
    /// then lets the read end: each Refresh holds the wait, and its
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

    /// A dropped review's wait says where its work runs in a tab that did
    /// not ask for it: the busy label, and the reason a refused control
    /// gives in its tooltip, as the palette does (`busy_reason`), name
    /// another tab's review, or a closed tab's once that tab has closed, in
    /// a tab that opens its repository again too, while the tab that asked
    /// shows the review's own label and the Git operation's reason. The
    /// next tab once read "Reviewing file changes…" as why its own actions
    /// were unavailable, a review it never asked for, and so did a tab that
    /// opened the closed tab's repository again, found by its path.
    #[gpui::test]
    async fn a_dropped_reviews_wait_says_where_it_runs(cx: &mut TestAppContext) {
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
        // The busy label drawn, the reason, and Projects' tooltip, once the
        // busy look shows.
        let shown = |cx: &mut VisualTestContext| {
            cx.executor().advance_clock(BUSY_LOOK_DELAY);
            draw(cx);
            assert!(cx.debug_bounds("operation-progress").is_some());
            let (label, reason) =
                app.read_with(cx, |app, _| (app.operation_shown(), app.busy_reason()));
            let tooltip = refusals::tooltip("projects").map(|tooltip| tooltip.to_string());
            (label, reason, tooltip)
        };
        let says = |label: &'static str, reason: &'static str| {
            (
                Some(label),
                reason,
                Some(format!("Open Projects · {reason}")),
            )
        };

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
            refusals::tooltip("projects").as_deref(),
            Some("Open Projects"),
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
