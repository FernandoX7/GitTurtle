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
            // Leaving cancelled a review's preparation before its end could
            // run the read Changes asked for meanwhile.
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
