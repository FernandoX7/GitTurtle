use super::*;
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::{cell::RefCell, path::Path, process::Command, rc::Rc};

struct Fixture {
    _directory: tempfile::TempDir,
    repo: GitRepository,
    first: Worktree,
    second: Worktree,
}

fn git(path: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "init.templateDir=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=Worktree fixture",
            "-c",
            "user.email=worktree@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        command.env_remove(name);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let main = directory.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "--initial-branch=main"]);
        std::fs::write(main.join("tracked.txt"), "original\n").unwrap();
        std::fs::write(main.join(".gitignore"), "*.ignored\n").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-m", "Fixture"]);
        for branch in ["first", "second"] {
            git(
                &main,
                &[
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    directory.path().join(branch).to_str().unwrap(),
                ],
            );
        }
        let repo = GitRepository::open(&main).unwrap();
        let trees = repo.worktrees().unwrap();
        let branch = |name| {
            trees
                .iter()
                .find(|tree| tree.branch.as_deref() == Some(name))
                .unwrap()
                .clone()
        };
        Self {
            first: branch("first"),
            second: branch("second"),
            repo,
            _directory: directory,
        }
    }
}

fn app_window(
    cx: &mut TestAppContext,
    repo: GitRepository,
) -> (Entity<GitTurtle>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        image_lifetime::init(cx);
        native_accessibility::bind_keys(cx);
    });
    let captured = Rc::new(RefCell::new(None));
    let observed = captured.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| {
            let mut app = GitTurtle::new(
                None,
                Preferences::default(),
                repository_tabs::Session::default(),
                activity::State::default(),
                recovery_drafts::State::default(),
                window,
                cx,
            );
            app.path = Some(repo.path().to_owned());
            app.repository = Some(repo);
            app.page = AppPage::Repository;
            app._display_preferences_task = None;
            app
        });
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    cx.simulate_resize(size(px(1480.), px(981.)));
    draw(cx);
    let app = observed.borrow_mut().take().unwrap();
    (app, cx)
}

fn manager(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Entity<WorktreeManager> {
    app.read_with(cx, |app, _| {
        app.worktree_management.draft.as_ref().unwrap().clone()
    })
}

async fn settle(manager: &Entity<WorktreeManager>, cx: &mut VisualTestContext) {
    // A targeted refresh first lists registrations, then inspects its captured
    // row. Drain both stages without depending on real-time polling.
    for _ in 0..4 {
        cx.executor().run_until_parked();
        let task = manager.update(cx, |manager, _| manager.task.take());
        let Some(task) = task else { break };
        task.await;
    }
    cx.executor().run_until_parked();
    assert!(!manager.read_with(cx, |manager, _| manager.pending));
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
    cx.executor().run_until_parked();
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    draw(cx);
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.executor().run_until_parked();
}

fn finish_dialog(cx: &mut VisualTestContext, cancel: bool) {
    draw(cx);
    assert!(cx.debug_bounds("dialog-layer").is_some());
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        if cancel {
            window.dispatch_action(Box::new(gpui_kit::component::dialog::Cancel), cx);
        } else {
            window.dispatch_action(
                Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
                cx,
            );
        }
    });
    cx.executor().run_until_parked();
    cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
}

fn assert_no_confirmation(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    draw(cx);
    assert!(cx.debug_bounds("operation-consequences").is_none());
    app.read_with(cx, |app, _| assert!(app.operation_busy.is_none()));
}

#[gpui::test]
async fn targeted_worktree_actions_restore_manage_and_cancel_exact_removal(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
    let retained = manager(&app, cx);
    settle(&retained, cx).await;
    click(cx, "worktree-create-tab");
    cx.update(|window, cx| {
        retained.update(cx, |manager, cx| {
            manager.filter.update(cx, |input, cx| {
                input.set_value("does not match the selected target", window, cx)
            });
            manager.branch.update(cx, |input, cx| {
                input.set_value("preserved-create-draft", window, cx)
            });
        });
    });
    finish_dialog(cx, false);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_worktree_actions(fixture.second.clone(), None, window, cx)
        });
    });
    let reopened = manager(&app, cx);
    assert_eq!(reopened.entity_id(), retained.entity_id());
    settle(&reopened, cx).await;
    reopened.read_with(cx, |manager, cx| {
        assert!(!manager.creating);
        assert_eq!(manager.filter.read(cx).value(), "");
        assert_eq!(manager.branch.read(cx).value(), "preserved-create-draft");
        assert_eq!(manager.selected.as_ref(), Some(&fixture.second.path));
        assert_eq!(manager.details.as_ref().unwrap().tree, fixture.second);
    });
    click(cx, "remove-managed-worktree");
    settle(&reopened, cx).await;
    draw(cx);
    assert!(cx.debug_bounds("operation-consequences").is_some());
    app.read_with(cx, |app, _| assert!(app.operation_busy.is_none()));
    finish_dialog(cx, true);
    assert_no_confirmation(&app, cx);
    assert!(fixture.second.path.join("tracked.txt").is_file());
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 3);
}

#[gpui::test]
async fn manager_removal_rechecks_changes_since_selection(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_worktree_actions(fixture.first.clone(), None, window, cx)
        });
    });
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    assert!(manager.read_with(cx, |manager, _| {
        manager.details.as_ref().unwrap().removal_blocked.is_none()
    }));
    std::fs::write(fixture.first.path.join("new.ignored"), "keep this data").unwrap();
    click(cx, "remove-managed-worktree");
    settle(&manager, cx).await;
    assert_no_confirmation(&app, cx);
    manager.read_with(cx, |manager, _| {
        let details = manager.details.as_ref().unwrap();
        assert_eq!(details.tree, fixture.first);
        assert_eq!(details.ignored_files, 1);
        assert!(details.removal_blocked.is_some());
    });
    assert_eq!(
        std::fs::read(fixture.first.path.join("new.ignored")).unwrap(),
        b"keep this data"
    );
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 3);
}

#[gpui::test]
async fn switching_manager_workflow_cancels_pending_removal_review(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    let (release, gate) = std::sync::mpsc::channel();
    let (started, ready) = futures::channel::oneshot::channel();
    let held = manager.read_with(cx, |manager, _| {
        manager.reader.submit_read(move || {
            let _ = started.send(());
            gate.recv_timeout(std::time::Duration::from_secs(10))?;
            Ok(())
        })
    });
    ready.await.unwrap();
    cx.update(|window, cx| {
        manager.update(cx, |manager, cx| {
            manager.inspect(
                fixture.first.clone(),
                Some(RemovalMode::Ordinary),
                window,
                cx,
            )
        });
    });
    assert!(manager.read_with(cx, |manager, _| manager.pending));
    click(cx, "worktree-create-tab");
    release.send(()).unwrap();
    held.await.unwrap().unwrap();
    settle(&manager, cx).await;
    assert_no_confirmation(&app, cx);
    manager.read_with(cx, |manager, _| {
        assert!(manager.creating);
        assert!(manager.details.is_none());
    });
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 3);
}

#[gpui::test]
async fn switching_to_create_preserves_ordinary_metadata_loading(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    let (release, gate) = std::sync::mpsc::channel();
    let (started, ready) = futures::channel::oneshot::channel();
    let held = manager.read_with(cx, |manager, _| {
        manager.reader.submit_read(move || {
            let _ = started.send(());
            gate.recv_timeout(std::time::Duration::from_secs(10))?;
            Ok(())
        })
    });
    ready.await.unwrap();
    cx.update(|window, cx| {
        manager.update(cx, |manager, cx| {
            manager.trees.clear();
            manager.branches.clear();
            manager.refresh(window, cx);
        });
    });
    click(cx, "worktree-create-tab");
    assert!(manager.read_with(cx, |manager, _| manager.pending));
    release.send(()).unwrap();
    held.await.unwrap().unwrap();
    settle(&manager, cx).await;
    assert_no_confirmation(&app, cx);
    manager.read_with(cx, |manager, _| {
        assert!(manager.creating);
        assert_eq!(manager.trees.len(), 3);
        assert!(manager.branches.iter().any(|branch| branch.name == "main"));
    });
}

#[gpui::test]
async fn keyboard_actions_review_focused_worktree_without_opening_it(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    let trees = fixture.repo.worktrees().unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.worktrees = trees;
            app.nav_mode = NavMode::Worktrees;
            app.rebuild_navigation(cx);
            app.nav_cursor = app.nav_rows.iter().position(|row| {
                matches!(row, NavRow::Worktree(index) if app.worktrees[*index] == fixture.second)
            });
            assert!(app.nav_cursor.is_some());
            window.focus(&app.nav_focus, cx);
        });
    });
    draw(cx);
    cx.simulate_keystrokes("shift-f10");
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    manager.read_with(cx, |manager, _| {
        assert_eq!(manager.selected.as_ref(), Some(&fixture.second.path));
        assert_eq!(manager.details.as_ref().unwrap().tree, fixture.second);
    });
    app.read_with(cx, |app, _| {
        assert_eq!(app.path.as_deref(), Some(fixture.repo.path()));
        assert_eq!(app.repository.as_ref().unwrap().path(), fixture.repo.path());
        assert!(app.operation_busy.is_none());
    });
    finish_dialog(cx, false);
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 3);
}

#[gpui::test]
async fn accepted_removal_keeps_captured_target_after_manager_closes(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_worktree_actions(
                fixture.second.clone(),
                Some(RemovalMode::Ordinary),
                window,
                cx,
            )
        });
    });
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    let (release, gate) = std::sync::mpsc::channel();
    let (started, ready) = futures::channel::oneshot::channel();
    let held = app.read_with(cx, |app, _| {
        app.operations.submit(move || {
            let _ = started.send(());
            gate.recv_timeout(std::time::Duration::from_secs(10))?;
            Ok(())
        })
    });
    ready.await.unwrap();
    finish_dialog(cx, false);
    app.update(cx, |app, _| app.worktree_management = State::default());
    drop(manager);
    app.read_with(cx, |app, _| {
        assert!(app.operation_busy.is_some());
        assert_eq!(
            app.operation_repository.as_deref(),
            Some(fixture.repo.path())
        );
    });
    assert!(fixture.second.path.is_dir());
    release.send(()).unwrap();
    held.await.unwrap().unwrap();
    let task = app.update(cx, |app, _| app.operation_task.take()).unwrap();
    task.await;
    cx.executor().run_until_parked();
    app.read_with(cx, |app, _| {
        assert!(app.operation_busy.is_none());
        assert!(app.operation_error.is_none(), "{:?}", app.operation_error);
        assert_eq!(app.path.as_deref(), Some(fixture.repo.path()));
        assert!(
            app.operation_notice
                .as_ref()
                .unwrap()
                .contains("Removed worktree")
        );
    });
    assert!(!fixture.second.path.exists());
    assert!(fixture.first.path.join("tracked.txt").is_file());
    assert!(fixture.repo.path().join("tracked.txt").is_file());
    let trees = fixture.repo.worktrees().unwrap();
    assert_eq!(trees.len(), 2);
    assert!(trees.iter().all(|tree| tree.path != fixture.second.path));
    assert!(
        fixture
            .repo
            .branches()
            .unwrap()
            .iter()
            .any(|branch| branch.name == "second")
    );
}

#[gpui::test]
async fn targeted_removal_refreshes_guards_and_disabled_control_cannot_confirm(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    let main = fixture
        .repo
        .worktrees()
        .unwrap()
        .into_iter()
        .find(|tree| tree.path == fixture.repo.path())
        .unwrap();
    // Ignored content only blocks ordinary removal; a lock or the main
    // worktree also blocks force removal, so those rows request force.
    for (target, guard, mode) in [
        (fixture.first.clone(), "ignored", RemovalMode::Ordinary),
        (fixture.second.clone(), "locked", RemovalMode::Force),
        (main, "main", RemovalMode::Force),
    ] {
        // Navigator rows predate these changes; invoking removal must inspect
        // the current filesystem/lock before deciding whether to confirm.
        if guard == "ignored" {
            std::fs::write(target.path.join("local.ignored"), "keep me").unwrap();
        } else if guard == "locked" {
            git(
                fixture.repo.path(),
                &["worktree", "lock", target.path.to_str().unwrap()],
            );
        }
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_worktree_actions(target.clone(), Some(mode), window, cx)
            });
        });
        let manager = manager(&app, cx);
        settle(&manager, cx).await;
        manager.read_with(cx, |manager, _| {
            assert_eq!(manager.selected.as_ref(), Some(&target.path));
            // Lock identity drift can reject the captured row outright; every
            // guard must keep the user out of a removal confirmation.
            assert!(
                manager.error.is_some()
                    || manager
                        .details
                        .as_ref()
                        .is_some_and(|details| mode.blocked(details).is_some()),
                "guard: {guard}"
            );
        });
        assert_no_confirmation(&app, cx);
        let blocked_controls: &[&'static str] = match mode {
            RemovalMode::Ordinary => &["remove-managed-worktree"],
            RemovalMode::Force => &["remove-managed-worktree", "force-remove-managed-worktree"],
        };
        for selector in blocked_controls {
            if cx.debug_bounds(selector).is_some() {
                click(cx, selector);
                assert_no_confirmation(&app, cx);
            }
        }
        finish_dialog(cx, false);
        assert!(target.path.exists());
    }
}

#[gpui::test]
async fn force_removal_confirms_dirty_target_that_ordinary_removal_refuses(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    std::fs::write(fixture.second.path.join("build.ignored"), "discard").unwrap();
    std::fs::write(fixture.second.path.join("notes.txt"), "discard").unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_worktree_actions(
                fixture.second.clone(),
                Some(RemovalMode::Ordinary),
                window,
                cx,
            )
        });
    });
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    assert_no_confirmation(&app, cx);
    manager.read_with(cx, |manager, _| {
        let details = manager.details.as_ref().unwrap();
        assert_eq!(details.tree, fixture.second);
        assert_eq!(details.changed_files, 1);
        assert_eq!(details.ignored_files, 1);
        assert!(details.removal_blocked.is_some());
        assert!(details.force_removal_blocked.is_none());
    });
    click(cx, "remove-managed-worktree");
    assert_no_confirmation(&app, cx);
    // The manager button reviews the exact target again, then confirms.
    click(cx, "force-remove-managed-worktree");
    settle(&manager, cx).await;
    draw(cx);
    assert!(cx.debug_bounds("operation-consequences").is_some());
    finish_dialog(cx, true);
    assert_no_confirmation(&app, cx);
    assert!(fixture.second.path.join("build.ignored").is_file());
    assert!(fixture.second.path.join("notes.txt").is_file());
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 3);

    // The navigator action opens the force confirmation for the captured row.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_worktree_actions(fixture.second.clone(), Some(RemovalMode::Force), window, cx)
        });
    });
    let reopened = manager.clone();
    settle(&reopened, cx).await;
    draw(cx);
    assert!(cx.debug_bounds("operation-consequences").is_some());
    finish_dialog(cx, false);
    let task = app.update(cx, |app, _| app.operation_task.take()).unwrap();
    task.await;
    cx.executor().run_until_parked();
    app.read_with(cx, |app, _| {
        assert!(app.operation_busy.is_none());
        assert!(app.operation_error.is_none(), "{:?}", app.operation_error);
        assert!(
            app.operation_notice
                .as_ref()
                .unwrap()
                .contains("Force removed worktree")
        );
    });
    assert!(!fixture.second.path.exists());
    assert!(fixture.first.path.join("tracked.txt").is_file());
    assert!(fixture.repo.path().join("tracked.txt").is_file());
    let trees = fixture.repo.worktrees().unwrap();
    assert_eq!(trees.len(), 2);
    assert!(trees.iter().all(|tree| tree.path != fixture.second.path));
    assert!(
        fixture
            .repo
            .branches()
            .unwrap()
            .iter()
            .any(|branch| branch.name == "second")
    );
}

#[gpui::test]
async fn stale_target_never_falls_back_to_another_registered_worktree(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    // A changed commit at the same path and an unregistered path are both
    // stale navigator identities, even when other removable worktrees remain.
    git(
        &fixture.first.path,
        &["commit", "--allow-empty", "-m", "Moved"],
    );
    git(
        fixture.repo.path(),
        &["worktree", "remove", fixture.second.path.to_str().unwrap()],
    );
    for target in [&fixture.first, &fixture.second] {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_worktree_actions(target.clone(), Some(RemovalMode::Ordinary), window, cx)
            });
        });
        let manager = manager(&app, cx);
        settle(&manager, cx).await;
        manager.read_with(cx, |manager, _| {
            assert_eq!(manager.selected.as_ref(), Some(&target.path));
            assert!(manager.details.is_none());
            assert!(manager.error.is_some());
        });
        assert_no_confirmation(&app, cx);
        finish_dialog(cx, false);
    }
    assert!(fixture.first.path.join("tracked.txt").is_file());
}

#[gpui::test]
async fn cancelled_or_superseded_target_read_cannot_open_removal_confirmation(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    for cancel in [true, false] {
        let (release, gate) = std::sync::mpsc::channel();
        let (started, ready) = futures::channel::oneshot::channel();
        let held = manager.read_with(cx, |manager, _| {
            manager.reader.submit_read(move || {
                let _ = started.send(());
                gate.recv_timeout(std::time::Duration::from_secs(10))?;
                Ok(())
            })
        });
        ready.await.unwrap();
        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.inspect(
                    fixture.first.clone(),
                    Some(RemovalMode::Ordinary),
                    window,
                    cx,
                )
            });
        });
        assert!(manager.read_with(cx, |manager, _| manager.pending));
        if cancel {
            click(cx, "cancel-worktree-read");
        } else {
            cx.update(|window, cx| {
                manager.update(cx, |manager, cx| {
                    manager.select(fixture.second.clone(), window, cx)
                });
            });
        }
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        settle(&manager, cx).await;
        assert_no_confirmation(&app, cx);
        manager.read_with(cx, |manager, _| {
            if cancel {
                assert!(manager.details.is_none());
            } else {
                assert_eq!(manager.selected.as_ref(), Some(&fixture.second.path));
                assert_eq!(manager.details.as_ref().unwrap().tree, fixture.second);
            }
        });
    }
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 3);
}
