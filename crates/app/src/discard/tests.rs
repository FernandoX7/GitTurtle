use super::*;
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::{cell::RefCell, path::Path, process::Command, rc::Rc};

struct Fixture {
    _directory: tempfile::TempDir,
    repo: GitRepository,
}

fn git_output(path: &Path, args: &[&str]) -> std::process::Output {
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
            "user.name=Discard fixture",
            "-c",
            "user.email=discard@example.invalid",
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
    command.output().unwrap()
}

fn git(path: &Path, args: &[&str]) {
    let output = git_output(path, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("work");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "--initial-branch=main"]);
        std::fs::write(root.join("tracked.txt"), "original\n").unwrap();
        std::fs::write(root.join("other.txt"), "other\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "Fixture"]);
        Self {
            repo: GitRepository::open(&root).unwrap(),
            _directory: directory,
        }
    }
    fn root(&self) -> &Path {
        self.repo.path()
    }
    fn entry(&self, path: &str) -> StatusEntry {
        self.repo
            .status()
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.path == Path::new(path))
            .unwrap_or_else(|| panic!("no status entry for {path}"))
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

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
    cx.executor().run_until_parked();
}

fn finish_dialog(cx: &mut VisualTestContext, cancel: bool) {
    draw(cx);
    assert!(cx.debug_bounds("dialog-layer").is_some());
    click(
        cx,
        if cancel {
            "cancel-discard"
        } else {
            "confirm-discard"
        },
    );
    cx.executor().run_until_parked();
    cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    draw(cx);
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.executor().run_until_parked();
}

async fn prepared(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let task = app
        .update(cx, |app, _| app.discard_actions.task.take())
        .expect("discard preparation task");
    task.await;
    cx.executor().run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("operation-consequences").is_some());
    app.read_with(cx, |app, _| assert!(app.operation_busy.is_none()));
}

async fn completed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let task = app
        .update(cx, |app, _| app.operation_task.take())
        .expect("operation task");
    task.await;
    cx.executor().run_until_parked();
    app.read_with(cx, |app, _| assert!(app.operation_busy.is_none()));
}

#[gpui::test]
async fn discard_reviews_exact_entry_and_restores_tracked_file_from_head(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let tracked = fixture.root().join("tracked.txt");
    std::fs::write(&tracked, "edited\n").unwrap();
    std::fs::write(fixture.root().join("other.txt"), "kept edit\n").unwrap();
    git(fixture.root(), &["add", "other.txt"]);
    std::fs::write(fixture.root().join("note.txt"), "untracked\n").unwrap();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    let entry = fixture.entry("tracked.txt");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.open_discard(entry.clone(), window, cx));
    });
    prepared(&app, cx).await;
    // A later edit invalidates the reviewed identity; nothing is restored.
    std::fs::write(&tracked, "edited again\n").unwrap();
    finish_dialog(cx, false);
    completed(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(
            app.operation_error
                .as_deref()
                .is_some_and(|error| error.contains("changed after review")),
            "{:?}",
            app.operation_error
        );
    });
    assert_eq!(std::fs::read(&tracked).unwrap(), b"edited again\n");

    let entry = fixture.entry("tracked.txt");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.open_discard(entry, window, cx));
    });
    prepared(&app, cx).await;
    // Tab backwards from the dialog container reaches its final action. The
    // actual focused button still accepts Enter after the user chooses it.
    cx.simulate_keystrokes("shift-tab enter");
    draw(cx);
    cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
    completed(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.operation_error.is_none(), "{:?}", app.operation_error);
        assert!(
            app.operation_notice
                .as_deref()
                .is_some_and(|notice| notice.contains("Discarded changes to tracked.txt"))
        );
    });
    assert_eq!(std::fs::read(&tracked).unwrap(), b"original\n");
    assert_eq!(
        std::fs::read(fixture.root().join("other.txt")).unwrap(),
        b"kept edit\n"
    );
    assert_eq!(
        fixture.entry("other.txt").staged,
        Some(gitturtle_core::ChangeStatus::Modified)
    );
    assert!(fixture.root().join("note.txt").is_file());
}

#[test]
fn review_names_deletion_for_added_and_intent_to_add_rows() {
    let fixture = Fixture::new();
    std::fs::write(fixture.root().join("staged.txt"), "staged\n").unwrap();
    git(fixture.root(), &["add", "staged.txt"]);
    std::fs::write(fixture.root().join("intent.txt"), "intent\n").unwrap();
    git(fixture.root(), &["add", "-N", "intent.txt"]);
    std::fs::write(fixture.root().join("tracked.txt"), "edited\n").unwrap();
    for (path, deletes) in [
        ("staged.txt", true),
        ("intent.txt", true),
        ("tracked.txt", false),
    ] {
        let entry = fixture.entry(path);
        assert!(!entry.untracked);
        let plan = fixture.repo.discard_plan(&entry).unwrap();
        let explanation = consequence(&plan);
        assert_eq!(
            explanation.contains("deleted from the working folder"),
            deletes,
            "{explanation}"
        );
        assert_eq!(
            explanation
                .contains("restored from the last commit in both the index and the working folder"),
            !deletes,
            "{explanation}"
        );
        fixture
            .repo
            .execute(&WriteCommand::Discard(Arc::new(plan)))
            .unwrap();
        assert_eq!(fixture.root().join(path).exists(), !deletes, "{path}");
    }
    assert_eq!(
        std::fs::read(fixture.root().join("tracked.txt")).unwrap(),
        b"original\n"
    );
    assert!(fixture.repo.status().unwrap().entries.is_empty());
}

#[gpui::test]
async fn discard_deletes_untracked_file_after_cancel_and_skips_conflicted_rows(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let scratch = fixture.root().join("scratch.txt");
    std::fs::write(&scratch, "scratch\n").unwrap();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    let entry = fixture.entry("scratch.txt");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.open_discard(entry.clone(), window, cx));
    });
    prepared(&app, cx).await;
    finish_dialog(cx, true);
    app.read_with(cx, |app, _| assert!(app.operation_busy.is_none()));
    assert!(scratch.is_file());
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.open_discard(entry, window, cx));
    });
    prepared(&app, cx).await;
    finish_dialog(cx, false);
    completed(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.operation_error.is_none(), "{:?}", app.operation_error);
        assert!(
            app.operation_notice
                .as_deref()
                .is_some_and(|notice| notice.contains("Deleted untracked file scratch.txt"))
        );
    });
    assert!(!scratch.exists());
    assert!(fixture.root().join("tracked.txt").is_file());

    git(fixture.root(), &["checkout", "-q", "-b", "side"]);
    std::fs::write(fixture.root().join("tracked.txt"), "side\n").unwrap();
    git(fixture.root(), &["commit", "-q", "-am", "Side"]);
    git(fixture.root(), &["checkout", "-q", "main"]);
    std::fs::write(fixture.root().join("tracked.txt"), "main\n").unwrap();
    git(fixture.root(), &["commit", "-q", "-am", "Main"]);
    assert!(
        !git_output(fixture.root(), &["merge", "side"])
            .status
            .success()
    );
    let conflicted = fixture.entry("tracked.txt");
    assert!(conflicted.conflicted);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.open_discard(conflicted, window, cx));
    });
    draw(cx);
    app.read_with(cx, |app, _| {
        assert!(app.operation_busy.is_none());
        assert!(app.discard_actions.task.is_none());
    });
    assert!(cx.debug_bounds("operation-consequences").is_none());
    assert!(fixture.entry("tracked.txt").conflicted);
}

#[gpui::test]
async fn discard_preparation_is_canceled_when_leaving_and_returning_to_repository(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    std::fs::write(fixture.root().join("tracked.txt"), "keep edit\n").unwrap();
    let (app, cx) = app_window(cx, fixture.repo.clone());
    for settings in [false, true] {
        let (release, gate) = std::sync::mpsc::channel();
        let blocked = app.update(cx, |app, _| {
            app.operations.submit_read(move || {
                gate.recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
                Ok(())
            })
        });
        let entry = fixture.entry("tracked.txt");
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(entry, window, cx)));
        // Retain the task to exercise the late callback's generation check too.
        let pending = app.update(cx, |app, _| app.discard_actions.task.take().unwrap());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                if settings {
                    app.show_settings(window, cx);
                } else {
                    app.show_projects(window, cx);
                }
                assert!(app.operation_busy.is_none());
                assert_ne!(app.page, AppPage::Repository);
                app.return_from_page(window, cx);
                assert_eq!(app.page, AppPage::Repository);
            });
        });
        release.send(()).unwrap();
        blocked.await.unwrap().unwrap();
        pending.await;
        draw(cx);
        cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        assert!(cx.debug_bounds("operation-consequences").is_none());
        assert_eq!(
            std::fs::read(fixture.root().join("tracked.txt")).unwrap(),
            b"keep edit\n"
        );
    }
}

#[gpui::test]
async fn discard_requires_the_reviewed_resolved_repository(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let other = Fixture::new();
    for fixture in [&fixture, &other] {
        std::fs::write(fixture.root().join("tracked.txt"), "keep edit\n").unwrap();
    }
    let (app, cx) = app_window(cx, fixture.repo.clone());
    let entry = fixture.entry("tracked.txt");
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_discard(entry, window, cx)));
    prepared(&app, cx).await;
    // Requested paths can lag the resolved repository during navigation.
    app.update(cx, |app, _| app.repository = Some(other.repo.clone()));
    finish_dialog(cx, false);
    app.read_with(cx, |app, _| assert!(app.operation_task.is_none()));
    for fixture in [&fixture, &other] {
        assert_eq!(
            std::fs::read(fixture.root().join("tracked.txt")).unwrap(),
            b"keep edit\n"
        );
    }
}

#[gpui::test]
async fn discard_review_keeps_long_paths_and_actions_reachable_without_implicit_enter(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new();
    let relative = PathBuf::from(format!(
        "{}/{}/{}.txt",
        "directory-with-spaces — ".repeat(5),
        "nested-directory-".repeat(9),
        "x".repeat(251)
    ));
    let absolute = fixture.root().join(&relative);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(&absolute, "keep scratch\n").unwrap();
    let entry = fixture.entry(relative.to_str().unwrap());
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
                app.update(cx, |app, cx| app.open_discard(entry.clone(), window, cx));
            });
            prepared(&app, cx).await;
            let warning = cx.debug_bounds("discard-warning").unwrap();
            let details = cx.debug_bounds("discard-details").unwrap();
            let path = cx.debug_bounds("discard-path").unwrap();
            let confirm = cx.debug_bounds("confirm-discard").unwrap();
            let cancel = cx.debug_bounds("cancel-discard").unwrap();
            assert!(warning.bottom() <= details.top());
            assert!(details.size.height >= px(100.), "font {font}: {details:?}");
            assert!(details.bottom() <= confirm.top());
            assert!(confirm.bottom() < px(680.) && cancel.bottom() < px(680.));
            assert!(path.left() >= details.left() && path.right() <= details.right());
            assert!(
                path.size.height > appearance::ui_size(24.),
                "path must wrap: {path:?}"
            );
            // A stray Enter on opening the review cannot erase a file.
            cx.simulate_keystrokes("enter");
            draw(cx);
            cx.update(|window, cx| assert!(window.has_active_dialog(cx)));
            assert!(absolute.is_file());
            click(cx, "copy-discard-path");
            cx.read(|cx| {
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().unwrap(),
                    absolute.display().to_string()
                )
            });
            click(cx, "discard-details");
            cx.simulate_keystrokes("end");
            draw(cx);
            let revalidation = cx.debug_bounds("discard-revalidation").unwrap();
            assert!(
                cx.debug_bounds("discard-details")
                    .unwrap()
                    .contains(&revalidation.center()),
                "review ending must be keyboard-reachable"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
            assert!(absolute.is_file());
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}
