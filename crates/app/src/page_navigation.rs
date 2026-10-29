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

impl GitTurtle {
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
        if self.mode != WorkspaceMode::History {
            self.ensure_editor(window, cx);
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
        let back = format!("Back to Projects · {}[", primary_label());
        assert_eq!(tooltip("page-back"), Some(back.clone()), "Back at first");
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        draw(cx);
        assert_eq!(
            tooltip("page-back"),
            Some(format!("{back} · {BUSY_REASON}")),
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
