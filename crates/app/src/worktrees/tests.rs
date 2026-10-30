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
        crate::views::tests::fixture_identity(&main);
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
    crate::views::tests::init_app(cx);
    cx.update(|cx| {
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
    // A dialog slides in on a wall-clock animation, so under load a control
    // can move between the frame a click takes its bounds from and the press,
    // and the click never lands. Open dialogs at rest; no worktree test covers
    // motion, and `click` refuses a control that is still moving.
    cx.update(|_, cx| cx.set_reduce_motion(true));
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

/// [`settle`] for `app`'s open Worktrees dialog, for tests in other modules.
pub(crate) async fn settle_open_manager(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let manager = manager(app, cx);
    settle(&manager, cx).await;
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
    cx.executor().run_until_parked();
}

/// Clicks `selector`'s control, which must be at rest. A control that moves
/// between frames, as one in a dialog still sliding in does, can move away
/// before a delayed press, which then misses it and leaves the state from
/// before the click: the symptom of the one loaded failure of
/// `manager_removal_rechecks_changes_since_selection`. Compared across two
/// frames, motion fails every run rather than only the loaded ones.
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    draw(cx);
    let bounds = cx.debug_bounds(selector).expect(selector);
    draw(cx);
    assert_eq!(
        cx.debug_bounds(selector),
        Some(bounds),
        "{selector} moved between frames"
    );
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
    // Shift-F10 opens the row's menu, the one a right-click opens, whose
    // Worktree actions… reviews it.
    cx.simulate_keystrokes("shift-f10");
    click(cx, "menu-item: Worktree actions… · MenuItem");
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

#[test]
fn force_review_distinguishes_preserved_branch_from_detached_commit() {
    let fixture = Fixture::new();
    let mut details = fixture.repo.worktree_details(&fixture.second).unwrap();
    let text = removal_review::kept_history(&details);
    assert!(text.contains("Branch ‘second’"), "{text}");
    assert!(
        text.contains("stays available to check out again"),
        "{text}"
    );
    details.tree.branch = None;
    let text = removal_review::kept_history(&details);
    assert!(text.contains("Create a branch or tag first"), "{text}");
    assert!(text.contains("not a permanent backup"), "{text}");
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

#[gpui::test]
async fn force_review_keeps_warning_target_and_actions_visible_at_large_text(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let parent = fixture._directory.path().to_path_buf();
    // Exercise the same alias resolution on Linux as macOS's temporary
    // directories, where /var may resolve through /private/var.
    #[cfg(unix)]
    let parent = {
        let alias = parent.join("linked-parent");
        std::os::unix::fs::symlink(&parent, &alias).unwrap();
        alias
    };
    let target = parent.join(format!(
        "worktree with spaces — {}",
        "long-review-target-".repeat(7)
    ));
    git(
        fixture.repo.path(),
        &[
            "worktree",
            "move",
            fixture.second.path.to_str().unwrap(),
            target.to_str().unwrap(),
        ],
    );
    // Git records the resolved destination, rather than the alias used to
    // create it. Compare and copy that same path throughout the review.
    let target = target.canonicalize().unwrap();
    let tree = fixture
        .repo
        .worktrees()
        .unwrap()
        .into_iter()
        .find(|tree| tree.path == target)
        .unwrap();
    for index in 0..20 {
        std::fs::write(
            target.join(format!(
                "{index:02}-{}.txt",
                "very-long-untracked-path-".repeat(6)
            )),
            "discard",
        )
        .unwrap();
        std::fs::write(target.join(format!("{index:02}-build.ignored")), "discard").unwrap();
    }
    let details = fixture.repo.worktree_details(&tree).unwrap();
    assert_eq!(details.changed_files, 20);
    assert_eq!(details.ignored_files, 20);
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.simulate_resize(size(px(1000.), px(680.)));
    for theme in [
        appearance::ThemeChoice::Daylight,
        appearance::ThemeChoice::Midnight,
    ] {
        for font in [13, 18] {
            cx.update(|window, cx| {
                theme.apply(Some(window), cx);
                appearance::apply_text_sizes(font, 12, window, cx);
                app.update(cx, |app, cx| {
                    app.confirm_force_worktree_removal(details.clone(), window, cx)
                });
            });
            draw(cx);
            let warning = cx.debug_bounds("worktree-force-warning").unwrap();
            let identity = cx.debug_bounds("worktree-force-target").unwrap();
            let body = cx.debug_bounds("worktree-force-effects").unwrap();
            let confirm = cx.debug_bounds("confirm-force-worktree-removal").unwrap();
            let cancel = cx.debug_bounds("cancel-force-worktree-removal").unwrap();
            assert!(warning.top() >= px(0.) && warning.bottom() <= identity.top());
            assert!(identity.bottom() <= body.top());
            assert!(body.size.height >= px(100.), "font {font}: {body:?}");
            assert!(body.bottom() <= confirm.top());
            assert!(confirm.bottom() < px(680.), "font {font}: {confirm:?}");
            assert!(cancel.bottom() < px(680.));
            assert!(confirm.left() >= px(0.) && confirm.right() <= px(1000.));
            // Each pass copies again rather than reading the first pass's copy.
            cx.update(|_, cx| {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("nothing copied".into()))
            });
            click(cx, "copy-worktree-removal-path");
            cx.read(|cx| {
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().unwrap(),
                    target.display().to_string()
                )
            });
            click(cx, "worktree-force-effects");
            cx.simulate_keystrokes("end");
            draw(cx);
            let keep = cx.debug_bounds("worktree-will-keep").unwrap();
            assert!(
                body.contains(&keep.center()),
                "Will keep must be reachable by keyboard: {body:?}, {keep:?}"
            );
            let scrolled_body = cx.debug_bounds("worktree-force-effects").unwrap();
            let fixed_warning = cx.debug_bounds("worktree-force-warning").unwrap();
            let fixed_identity = cx.debug_bounds("worktree-force-target").unwrap();
            // Only the effect list's contents move; the warning and target stay
            // fixed in the dialog.
            assert_eq!(fixed_warning.size, warning.size);
            assert_eq!(fixed_identity.size, identity.size);
            assert_eq!(
                scrolled_body.top() - fixed_warning.top(),
                body.top() - warning.top()
            );
            assert_eq!(
                scrolled_body.top() - fixed_identity.top(),
                body.top() - identity.top()
            );
            assert!(
                cx.debug_bounds("confirm-force-worktree-removal")
                    .unwrap()
                    .bottom()
                    < px(680.)
            );
            click(cx, "cancel-force-worktree-removal");
            assert_no_confirmation(&app, cx);
            assert!(target.join("00-build.ignored").is_file());
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

#[gpui::test]
async fn force_blocker_is_visible_and_disabled_control_preserves_nested_repository(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let nested = fixture.second.path.join("archive.ignored");
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "--bare"]);
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_worktree_actions(fixture.second.clone(), None, window, cx)
        });
    });
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    manager.read_with(cx, |manager, _| {
        let details = manager.details.as_ref().unwrap();
        assert!(details.removal_blocked.is_some());
        assert!(
            details
                .force_removal_blocked
                .as_ref()
                .is_some_and(|reason| reason.contains("nested Git repository"))
        );
    });
    draw(cx);
    assert!(
        cx.debug_bounds("worktree-force-removal-protection")
            .is_some()
    );
    assert!(
        cx.debug_bounds("worktree-removal-protection").is_none(),
        "Do not recommend force removal when force removal is blocked"
    );
    click(cx, "force-remove-managed-worktree");
    assert_no_confirmation(&app, cx);
    finish_dialog(cx, false);
    assert!(nested.join("HEAD").is_file());
}

#[gpui::test]
async fn force_confirmation_cannot_follow_a_changed_repository_context(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    std::fs::write(fixture.second.path.join("keep.ignored"), "keep me").unwrap();
    let details = fixture.repo.worktree_details(&fixture.second).unwrap();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.confirm_force_worktree_removal(details, window, cx)
        });
    });
    draw(cx);
    app.update(cx, |app, _| {
        app.path = Some(fixture.first.path.clone());
        app.repository = Some(GitRepository::open(&fixture.first.path).unwrap());
    });
    finish_dialog(cx, false);
    assert_no_confirmation(&app, cx);
    assert_eq!(
        std::fs::read(fixture.second.path.join("keep.ignored")).unwrap(),
        b"keep me"
    );
    assert!(fixture.first.path.join("tracked.txt").is_file());
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 3);
}

/// The focus sweep (`focus_ring::sweep`) over the forced removal review of a
/// dirty worktree, at the default and the largest interface size.
#[gpui::test]
async fn the_forced_removal_review_draws_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    use crate::focus_ring::sweep;
    let fixture = Fixture::new();
    std::fs::write(fixture.second.path.join("notes.txt"), "discard").unwrap();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    sweep::prepare(&app, cx);
    let mut failures = Vec::new();
    for interface in sweep::SIZES {
        sweep::interface_size(cx, interface);
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
        click(cx, "force-remove-managed-worktree");
        settle(&manager, cx).await;
        draw(cx);
        assert!(cx.debug_bounds("operation-consequences").is_some());
        let effects = cx.debug_bounds("worktree-force-effects").unwrap();
        let rings = sweep::walk(
            &app,
            cx,
            "the forced removal review",
            interface,
            &mut failures,
        );
        // The effects scroll by key, so Tab stops there and rings their edge.
        assert!(
            sweep::rings_edge(&rings, effects),
            "{interface} pt: Tab rings the effects {effects:?}: {rings:?}"
        );
        sweep::close_dialogs(cx, "the forced removal review", interface, &mut failures);
    }
    sweep::assert_whole(failures);
}

/// In a 461 × 490 half tile, at 13 and 18 pt, with a worktree selected,
/// every Tab stop of the worktree manager draws its ring whole and in view
/// while the pointer moves: the content scrolls the control the keyboard
/// moves onto into view with its ring, the list as a whole for a row, as the
/// Reflog's and Stashes' do. The selected worktree's details, Open in
/// GitTurtle to Force remove, lay below the content's fold at both sizes,
/// where only the first field was revealed as the manager opened, so Tab
/// moved onto controls the fold cut or no frame showed.
#[gpui::test]
async fn a_narrow_manager_shows_every_detail_stop_whole(cx: &mut TestAppContext) {
    use crate::focus_ring::sweep;
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    let mut failures = Vec::new();
    for interface in sweep::SIZES {
        sweep::interface_size(cx, interface);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_worktree_actions(fixture.second.clone(), None, window, cx)
            })
        });
        let manager = manager(&app, cx);
        settle(&manager, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("force-remove-managed-worktree").is_some(),
            "{interface} pt: the selected worktree's details are drawn"
        );
        sweep::walk(
            &app,
            cx,
            "the narrow worktree manager",
            interface,
            &mut failures,
        );
        sweep::close_dialogs(cx, "the narrow worktree manager", interface, &mut failures);
    }
    sweep::assert_whole(failures);
}

/// In a 461 × 490 half tile, at 13 and 18 pt, every Tab stop of the
/// manager's Create form draws its ring whole and in view, walked with Tab
/// and then back with Shift+Tab, in its New branch form and in its Existing
/// branch form, whose branch choices, more than their list shows at once,
/// scroll on their own.
#[gpui::test]
async fn a_narrow_create_form_shows_every_stop_whole(cx: &mut TestAppContext) {
    use crate::focus_ring::sweep;
    let fixture = Fixture::new();
    // Local branches free to check out, more than the choices list shows.
    for index in 0..8 {
        git(fixture.repo.path(), &["branch", &format!("topic-{index}")]);
    }
    let (app, cx) = app_window(cx, fixture.repo.clone());
    sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    let mut failures = Vec::new();
    for interface in sweep::SIZES {
        sweep::interface_size(cx, interface);
        for (existing, surface) in [
            (false, "the narrow Create form, New branch"),
            (true, "the narrow Create form, Existing branch"),
        ] {
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
            let manager = manager(&app, cx);
            settle(&manager, cx).await;
            cx.update(|window, cx| {
                manager.update(cx, |manager, cx| {
                    manager.switch_form(true, window, cx);
                    manager.new_branch = !existing;
                })
            });
            draw(cx);
            draw(cx);
            assert!(
                cx.debug_bounds("worktree-branch-choices").is_some() == existing,
                "{interface} pt: {surface} shows its branch choices only for an existing branch"
            );
            sweep::walk(&app, cx, surface, interface, &mut failures);
            if existing {
                // Tab reached the free choices below the list's fold.
                let offset =
                    manager.read_with(cx, |manager, _| manager.choices.scroll().offset().y);
                assert!(
                    offset < px(0.),
                    "{interface} pt: Tab scrolled the branch choices"
                );
            }
            sweep::walk_back(&app, cx, surface, interface, &mut failures);
            sweep::close_dialogs(cx, surface, interface, &mut failures);
        }
    }
    sweep::assert_whole(failures);
}

/// The branch choices' scroll stays with the manager, as the worktree
/// list's does, so the choices kept the offset a reveal or a wheel left
/// them at while they were hidden. Each time they show again, after New
/// branch and Existing branch or Manage and Create worktree…, they start at
/// their first choice.
#[gpui::test]
async fn the_branch_choices_show_again_from_their_first_choice(cx: &mut TestAppContext) {
    use crate::focus_ring::sweep;
    let fixture = Fixture::new();
    for index in 0..8 {
        git(fixture.repo.path(), &["branch", &format!("topic-{index}")]);
    }
    let (app, cx) = app_window(cx, fixture.repo.clone());
    sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    click(cx, "worktree-create-tab");
    click(cx, "worktree-existing-branch");
    let offset = |cx: &mut VisualTestContext| {
        manager.read_with(cx, |manager, _| manager.choices.scroll().offset().y)
    };
    let scroll_to_end = |cx: &mut VisualTestContext| {
        manager.update(cx, |manager, _| {
            let scroll = manager.choices.scroll();
            scroll.set_offset(point(px(0.), -scroll.max_offset().y));
        });
        cx.update(|window, _| window.refresh());
        draw(cx);
        assert!(offset(cx) < px(0.), "the choices scroll");
    };
    let first = |cx: &mut VisualTestContext, at: &str| {
        assert_eq!(offset(cx), px(0.), "{at}: the choices start at the top");
        let list = cx
            .debug_bounds("worktree-branch-choices")
            .expect("the choices");
        let choice = cx
            .debug_bounds("worktree-branch-choice-0")
            .expect("the first choice");
        assert!(
            choice.top() >= list.top(),
            "{at}: the first choice {choice:?} shows in {list:?}"
        );
    };
    scroll_to_end(cx);
    click(cx, "worktree-new-branch");
    click(cx, "worktree-existing-branch");
    first(cx, "after New branch");
    scroll_to_end(cx);
    click(cx, "worktree-browse-tab");
    click(cx, "worktree-create-tab");
    first(cx, "after Manage");
}

/// With a worktree's details shown in a 461 × 490 half tile, a filter that
/// matches no worktree leaves the list its whole explanation at 13 and 18
/// pt, though the details run past the content's fold.
#[gpui::test]
async fn a_narrow_manager_shows_its_whole_no_match_line(cx: &mut TestAppContext) {
    use crate::focus_ring::sweep;
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    for interface in sweep::SIZES {
        sweep::interface_size(cx, interface);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_worktree_actions(fixture.second.clone(), None, window, cx)
            })
        });
        let manager = manager(&app, cx);
        settle(&manager, cx).await;
        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.filter.update(cx, |input, cx| {
                    input.set_value("no such worktree", window, cx)
                })
            })
        });
        draw(cx);
        draw(cx);
        assert!(
            cx.debug_bounds("worktree-selected-identity").is_some(),
            "{interface} pt: the details stay shown"
        );
        let line = cx
            .debug_bounds("worktrees-empty")
            .unwrap_or_else(|| panic!("{interface} pt: the no-match line"));
        let list = cx
            .debug_bounds("managed-worktree-list")
            .unwrap_or_else(|| panic!("{interface} pt: the list"));
        assert!(
            line.top() >= list.top() - px(0.01) && line.bottom() <= list.bottom() + px(0.01),
            "{interface} pt: the no-match line {line:?} lies whole in the list {list:?}"
        );
        sweep::close_dialogs(cx, "the filtered manager", interface, &mut Vec::new());
    }
}

/// A filter that matches no worktree says so as Tags says a filter matched
/// no tag: one muted line, the shared empty state's, with no closing period,
/// at the list's text inset. It was drawn in primary text and ended in a
/// period, unlike "No tags match this filter" beside it.
#[gpui::test]
async fn the_no_match_line_reads_as_the_tags_list_does(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    git(fixture.repo.path(), &["tag", "v1.0"]);
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    let muted = cx.read(|cx| gpui_kit::Hsla::from(rgb(palette(cx).muted)));
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    cx.update(|window, cx| {
        manager.update(cx, |manager, cx| {
            manager.filter.update(cx, |input, cx| {
                input.set_value("no such worktree", window, cx)
            })
        })
    });
    draw(cx);
    let list = cx
        .debug_bounds("managed-worktree-list")
        .expect("the worktree list");
    let line = cx
        .debug_bounds("worktrees-empty-line")
        .expect("the no-match line is the shared empty state's");
    assert_eq!(
        crate::empty_state::shown("worktrees-empty").as_deref(),
        Some("No worktrees match this filter")
    );
    let worktrees = crate::appearance::drawn_color("worktrees-empty-line");
    assert_eq!(worktrees, Some(muted), "the worktree line is muted");
    // Inside the list's padding, the ring's room, as the rows are.
    let room = cx.read(crate::appearance::button_ring_room);
    assert!(
        (line.left() - list.left() - room - TEXT_INSET).abs() <= px(0.5),
        "the line {line:?} starts at the list's text inset in {list:?}"
    );
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx);

    cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
    crate::tags::tests::read_lands(&app, cx).await;
    draw(cx);
    // Tags opens with its filter focused.
    cx.simulate_input("no such tag");
    draw(cx);
    let tags = crate::empty_state::shown("tag-list-empty");
    assert_eq!(tags.as_deref(), Some("No tags match this filter"));
    assert!(
        !tags.unwrap().ends_with('.')
            && !crate::empty_state::shown("worktrees-empty")
                .unwrap()
                .ends_with('.'),
        "neither line ends in a period"
    );
    assert_eq!(
        worktrees,
        crate::appearance::drawn_color("tag-list-empty-line"),
        "the worktree line is drawn in the tags line's color"
    );
}

/// A read that answers while keyboard focus is on its loading row's Cancel
/// takes the row away and hands focus to the first field below it, ringed
/// whole: the filter in Manage, the branch field in Create. Escape still
/// closes the manager. Focus stayed on the Cancel no frame drew, where
/// Escape reached nothing.
#[gpui::test]
async fn a_read_landing_while_its_cancel_holds_focus_moves_focus_to_the_first_field(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
    let manager = manager(&app, cx);
    settle(&manager, cx).await;
    draw(cx);
    for creating in [false, true] {
        cx.update(|_, cx| manager.update(cx, |this, cx| this.set_creating(creating, cx)));
        // Refresh while the manager's thread is held, as by a slow read, with
        // keyboard focus on the loading row's Cancel.
        let held = manager.read_with(cx, |this, _| this.reader.hold());
        cx.update(|window, cx| manager.update(cx, |this, cx| this.refresh(window, cx)));
        draw(cx);
        let cancel = manager.read_with(cx, |this, _| this.cancel_read.handle().clone());
        cx.update(|window, cx| window.focus(&cancel, cx));
        draw(cx);
        assert!(cx.debug_bounds("cancel-worktree-read").is_some());
        assert!(
            !cx.update(|window, _| window.context_stack().is_empty()),
            "the Cancel holds focus, drawn"
        );

        drop(held);
        settle(&manager, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("cancel-worktree-read").is_none(),
            "the reply takes the loading row away"
        );
        let (field, text) = cx.read(|cx| {
            let this = manager.read(cx);
            let field = if creating { &this.branch } else { &this.filter }.read(cx);
            (field.focus_handle(cx), field.input_bounds())
        });
        let name = if creating {
            "the branch field"
        } else {
            "the filter"
        };
        assert!(
            cx.update(|window, _| field.is_focused(window)),
            "focus moves from the Cancel to {name}"
        );
        assert!(
            !cx.update(|window, _| window.context_stack().is_empty()),
            "the frame draws {name}, which holds focus"
        );
        focus_ring::assert_input_ringed(cx, text, name);
    }
    cx.simulate_keystrokes("escape");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    draw(cx);
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "Escape closes the manager"
    );
}

/// The manager's error line is said once, at once, from the window's alert,
/// in the frame that draws it in place: a folder name of two parts refuses
/// Review create. Drawing the line again says nothing more, and the same
/// press refused again is said again, from a fresh node. The line was no
/// live region, and nothing was said.
#[gpui::test]
async fn the_managers_error_is_said_as_its_line_appears(cx: &mut TestAppContext) {
    use crate::copy_feedback::tests::alerted;
    use crate::views::tests::draw;
    let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_worktree_manager(window, cx)));
    settle_manager(&app, cx).await;
    let manager = app
        .read_with(cx, |app, _| app.worktree_management.draft.clone())
        .expect("the manager opens");
    cx.update(|window, cx| {
        manager.update(cx, |manager, cx| {
            manager
                .folder
                .update(cx, |folder, cx| folder.set_value("one/two", window, cx))
        })
    });
    draw(cx);
    assert_eq!(alerted(cx), None, "nothing is said before a press");
    let press = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| manager.prepare_create(window, cx))
        });
        draw(cx);
    };
    press(cx);
    let error = manager
        .read_with(cx, |manager, _| manager.error.clone())
        .expect("two parts are refused");
    assert!(
        cx.debug_bounds("worktree-error").is_some(),
        "the line stands in place"
    );
    let (first, said) = alerted(cx).expect("the frame that draws the line says it");
    assert_eq!(said, error);
    draw(cx);
    assert_eq!(
        alerted(cx).map(|(id, _)| id),
        Some(first.clone()),
        "drawing the line again says nothing more"
    );
    press(cx);
    let (second, said) = alerted(cx).expect("refused again, it is said again");
    assert!(
        second != first && said == error,
        "a fresh node says it again"
    );
}
