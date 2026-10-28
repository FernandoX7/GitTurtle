//! Git writes confirm on the control that asked for them (`write_from`): a
//! file row's Stage and Unstage on its row where the file arrives, Stage all,
//! Fetch, Pull and Push, Create and Switch on themselves, and a switch from
//! the branch picker's menu on the picker. A quiet refresh, a write with no
//! control and a failure confirm nothing.
use super::confirmations;
use crate::copy_feedback::{
    RESTORE_AFTER,
    tests::{announced, click, wait},
};
use crate::views::tests::{draw, fixture_git, history_window, settle};
use crate::*;
// `crate::*` brings GPUI's `test`, which `#[gpui::test]` would expand again.
use ::core::prelude::v1::test;
use gitturtle_core::ChangeArea;
use std::{path::Path, time::Duration};

/// What control `id` drew in the last frame: `Some(false)` its content at
/// rest, `Some(true)` its confirmation `state`, `None` neither.
fn confirms(cx: &mut VisualTestContext, id: &str, state: &str) -> Option<bool> {
    // The test context takes only static selectors.
    let mut drawn = |state: &str| cx.debug_bounds(format!("{id}/{state}").leak());
    match (drawn("rest"), drawn(state)) {
        (Some(_), None) => Some(false),
        (None, Some(_)) => Some(true),
        (None, None) => None,
        both => panic!("{id} drew both: {both:?}"),
    }
}

fn bounds(cx: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    cx.debug_bounds(selector.to_owned().leak())
        .unwrap_or_else(|| panic!("{selector} is drawn"))
}

/// Runs what is ready, without moving simulated time, until no write or
/// status read is pending and `done` holds of the frame then drawn: the
/// operation and read executors answer on their own threads, and a
/// confirmation's 1.5 s must not pass while the test waits for it. It draws
/// only once nothing is pending, so waiting on Git leaves the processor to
/// the tests beside it.
fn finish(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..2000 {
        cx.run_until_parked();
        if idle(app, cx) {
            draw(cx);
            if done(cx) {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("{what} never happened");
}

fn idle(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> bool {
    app.read_with(cx, |app, _| {
        app.operation_busy.is_none() && app.status_task.is_none()
    })
}

/// The fixture's worktree with its one commit checked out, a changed
/// `notes.txt` and a new `other.txt`, shown in Changes.
async fn working(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    &mut VisualTestContext,
    Option<std::path::PathBuf>,
) {
    let (fixture, app, cx) = history_window(cx).await;
    let worktree = fixture.path().join("narrow");
    fixture_git(&worktree, &["reset", "--hard", "--quiet"]);
    std::fs::write(worktree.join("notes.txt"), "changed\n").unwrap();
    std::fs::write(worktree.join("other.txt"), "other\n").unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    let repository = app.read_with(cx, |app, _| app.path.clone());
    (fixture, app, cx, repository)
}

/// A file row's Stage reads Staged on that file's row where it arrives, the
/// Staged row's control, as an icon in its own 28 px square; the row left
/// behind and the other file's row stay at rest. "Staged notes.txt" is
/// announced once, the status line records it and the window-wide notice
/// stays down; 1.5 s later the control is at rest. Unstage confirms the same
/// way on the row in Unstaged. Before this, the control kept its icon and the
/// outcome went only to the window-wide notice.
#[gpui::test]
async fn a_rows_stage_and_unstage_confirm_on_its_row_where_the_file_arrives(
    cx: &mut TestAppContext,
) {
    let (_fixture, app, cx, repository) = working(cx).await;
    let notes = |area| confirmations::file(&repository, Path::new("notes.txt"), area);
    let control = |area: ChangeArea| notes(area).control().to_string();
    let other = confirmations::file(&repository, Path::new("other.txt"), ChangeArea::Unstaged)
        .control()
        .to_string();
    let (unstaged, staged) = (control(ChangeArea::Unstaged), control(ChangeArea::Staged));
    assert_eq!(confirms(cx, &unstaged, "unstaged"), Some(false));
    assert_eq!(confirms(cx, &staged, "staged"), None, "not staged yet");
    assert_eq!(announced(cx), None);
    let square = bounds(cx, &format!("{unstaged}/rest")).size;

    let rest = bounds(cx, &format!("{unstaged}/rest"));
    cx.simulate_click(rest.center(), Modifiers::default());
    finish(&app, cx, "notes.txt arriving in Staged", |cx| {
        confirms(cx, &staged, "staged").is_some()
    });
    assert_eq!(
        confirms(cx, &staged, "staged"),
        Some(true),
        "on its new row"
    );
    assert_eq!(confirms(cx, &unstaged, "unstaged"), None, "the row it left");
    assert_eq!(
        confirms(cx, &other, "unstaged"),
        Some(false),
        "another file"
    );
    assert_eq!(bounds(cx, &format!("{staged}/staged")).size, square);
    assert_eq!(announced(cx).unwrap().1, "Staged notes.txt");
    app.read_with(cx, |app, _| assert_eq!(app.operation_notice, None));
    assert!(cx.debug_bounds("operation-notice-summary").is_none());

    wait(cx, RESTORE_AFTER - Duration::from_millis(1));
    assert_eq!(confirms(cx, &staged, "staged"), Some(true));
    wait(cx, Duration::from_millis(1));
    assert_eq!(confirms(cx, &staged, "staged"), Some(false));
    assert_eq!(announced(cx), None);

    let rest = bounds(cx, &format!("{staged}/rest"));
    cx.simulate_click(rest.center(), Modifiers::default());
    finish(&app, cx, "notes.txt returning to Unstaged", |cx| {
        confirms(cx, &unstaged, "unstaged").is_some()
    });
    assert_eq!(confirms(cx, &unstaged, "unstaged"), Some(true));
    assert_eq!(confirms(cx, &other, "unstaged"), Some(false));
    assert_eq!(announced(cx).unwrap().1, "Unstaged notes.txt");
    wait(cx, RESTORE_AFTER);
    assert_eq!(confirms(cx, &unstaged, "unstaged"), Some(false));
}

/// Stage all reads Staged after its check in its own bounds, its label's
/// place, though it is then disabled with nothing left to stage, and is
/// announced once without the window-wide notice; it reverts after 1.5 s.
/// A quiet refresh that finds the files unstaged again, and a Stage all
/// asked for without the control, confirm nothing on it: the latter keeps
/// the window-wide notice.
#[gpui::test]
async fn stage_all_confirms_in_its_own_bounds_and_nothing_else_confirms_it(
    cx: &mut TestAppContext,
) {
    let (fixture, app, cx, _repository) = working(cx).await;
    // Tall enough that the window-wide notice leaves every row in view.
    cx.simulate_resize(size(px(1000.), px(900.)));
    draw(cx);
    let rest = bounds(cx, "stage-all").size;
    assert_eq!(confirms(cx, "stage-all", "staged"), Some(false));
    assert!(
        cx.debug_bounds("stage-all/rest/text").is_some(),
        "Stage all"
    );

    click(cx, "stage-all");
    finish(&app, cx, "Stage all", |cx| {
        confirms(cx, "stage-all", "staged") == Some(true)
    });
    assert!(cx.debug_bounds("stage-all/staged/text").is_some(), "Staged");
    // Its heading moves down as the files join Staged above it.
    assert_eq!(bounds(cx, "stage-all").size, rest, "in its own bounds");
    assert_eq!(confirms(cx, "unstage-all", "unstaged"), Some(false));
    assert_eq!(announced(cx).unwrap().1, "Staged all working changes");
    app.read_with(cx, |app, _| {
        assert_eq!(app.operation_notice, None);
        assert_eq!(app.working_summary(), "2 working changes · 2 staged");
    });
    wait(cx, RESTORE_AFTER);
    assert_eq!(confirms(cx, "stage-all", "staged"), Some(false));
    assert_eq!(announced(cx), None);

    // Unstaged outside GitTurtle: the quiet refresh that reads it confirms
    // nothing.
    fixture_git(&fixture.path().join("narrow"), &["reset", "--quiet"]);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.queue_automatic_refresh(
                local_refresh::LocalChange {
                    git: true,
                    worktree: true,
                    ..Default::default()
                },
                window,
                cx,
            )
        })
    });
    finish(&app, cx, "the quiet refresh", |cx| {
        app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .is_some_and(|status| status.entries.iter().all(|e| e.staged.is_none()))
        })
    });
    settle(&app, cx).await;
    assert_eq!(confirms(cx, "stage-all", "staged"), Some(false));
    assert_eq!(confirms(cx, "unstage-all", "unstaged"), Some(false));
    assert_eq!(announced(cx), None);

    // Asked for elsewhere, with no control to confirm on.
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
    finish(&app, cx, "the unmarked Stage all", |cx| {
        app.read_with(cx, |app, _| app.operation_notice.is_some())
            && confirms(cx, "stage-all", "staged").is_some()
    });
    assert_eq!(confirms(cx, "stage-all", "staged"), Some(false));
    assert_eq!(announced(cx), None);
    app.read_with(cx, |app, _| {
        assert_eq!(
            app.operation_notice.as_deref(),
            Some("Staged all working changes")
        )
    });
}

/// The fixture with `origin`, a bare repository beside it that holds `main`
/// as its upstream, read into the action bar's targets.
async fn with_remote(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
    let (fixture, app, cx) = history_window(cx).await;
    let worktree = fixture.path().join("narrow");
    let remote = fixture.path().join("remote.git");
    fixture_git(
        fixture.path(),
        &[
            "init",
            "--bare",
            "--quiet",
            "--initial-branch=main",
            remote.to_str().unwrap(),
        ],
    );
    fixture_git(&worktree, &["reset", "--hard", "--quiet"]);
    fixture_git(
        &worktree,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    fixture_git(
        &worktree,
        &["push", "--quiet", "--set-upstream", "origin", "main"],
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    settle(&app, cx).await;
    app.read_with(cx, |app, cx| {
        assert_eq!(app.remote_name.read(cx).value(), "origin");
        assert_eq!(app.remote_branch.read(cx).value(), "main");
    });
    (fixture, app, cx)
}

/// Fetch, Pull and Push each read their past tense where they were clicked
/// once Git reports success: the word alone, from where the icon starts and
/// inside the content's own width, so their bounds, which decide where the
/// bar wraps, are those they had before they confirmed anything (an icon, a
/// gap and the label at the medium kit Button's insets). Each is announced
/// once ("Fetched origin"), recorded in the status line and kept out of the
/// window-wide notice, and reverts after 1.5 s.
#[gpui::test]
async fn fetch_pull_and_push_confirm_on_their_buttons(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = with_remote(cx).await;
    for (id, state, word, outcome) in [
        ("fetch", "fetched", "Fetched", "Fetched origin"),
        ("pull", "pulled", "Pulled", "Pulled origin/main"),
        ("push", "pushed", "Pushed", "Pushed main to origin/main"),
    ] {
        let rest = bounds(cx, id);
        assert_eq!(confirms(cx, id, state), Some(false), "{id}");
        // Its icon and label alone make its width: nothing is held for the
        // word it reads later.
        let content = bounds(cx, &format!("{id}/rest"));
        assert_eq!(
            content.left() - rest.left(),
            rest.right() - content.right(),
            "{id} at rest {rest:?} is its content {content:?} and its insets"
        );

        click(cx, id);
        finish(&app, cx, id, |cx| confirms(cx, id, state) == Some(true));
        assert_eq!(bounds(cx, id), rest, "{id} keeps its bounds");
        let done = bounds(cx, &format!("{id}/{state}/text"));
        assert_eq!(done.left(), content.left(), "{word} starts at the icon");
        assert!(
            done.right() <= content.right(),
            "{word} {done:?} in {content:?}"
        );
        assert_eq!(
            bounds(cx, &format!("{id}/{state}")).size.width,
            done.size.width,
            "{word} alone, without the check"
        );
        assert_eq!(announced(cx).unwrap().1, outcome);
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_notice, None);
            assert_eq!(app.operation_error, None);
            assert_eq!(app.status, outcome);
        });
        assert!(cx.debug_bounds("operation-notice-summary").is_none());
        for other in ["fetch", "pull", "push"]
            .into_iter()
            .filter(|other| *other != id)
        {
            assert!(cx.debug_bounds(format!("{other}/rest").leak()).is_some());
        }

        wait(cx, RESTORE_AFTER - Duration::from_millis(1));
        assert_eq!(confirms(cx, id, state), Some(true), "{id}");
        wait(cx, Duration::from_millis(1));
        assert_eq!(confirms(cx, id, state), Some(false), "{id}");
        assert_eq!(announced(cx), None);
    }
}

/// A Fetch that fails stays at rest and announces no success: its error
/// takes the window-wide error as before.
#[gpui::test]
async fn a_failed_fetch_never_reads_fetched(cx: &mut TestAppContext) {
    let (fixture, app, cx) = with_remote(cx).await;
    std::fs::remove_dir_all(fixture.path().join("remote.git")).unwrap();
    click(cx, "fetch");
    finish(&app, cx, "the failed fetch", |cx| {
        app.read_with(cx, |app, _| app.operation_error.is_some())
    });
    assert_eq!(confirms(cx, "fetch", "fetched"), Some(false));
    assert_eq!(announced(cx), None);
    assert!(cx.debug_bounds("operation-error-summary").is_some());
}

/// In the compact chrome (18 pt in 461 × 490) Fetch is its icon in a 28 px
/// square: it takes the check there and says Fetched in its tooltip while it
/// lasts, and keeps its square and its name ("Fetch: Fetch from origin").
#[gpui::test]
async fn compact_fetch_says_fetched_in_its_tooltip_and_keeps_its_name(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = with_remote(cx).await;
    cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    let square = bounds(cx, "fetch");
    let control = cx.update(|window, _| window.pixel_snap(appearance::ui_size(28.)));
    assert_eq!(square.size, size(control, control), "an icon");
    let name = || crate::views::COMPACT_LABELS.with(|names| names.borrow().get("fetch").cloned());
    assert_eq!(name().as_deref(), Some("Fetch: Fetch from origin"));
    let tooltip = |cx: &mut VisualTestContext| match (
        cx.debug_bounds("fetch/tooltip/rest"),
        cx.debug_bounds("fetch/tooltip/fetched"),
    ) {
        (None, None) => None,
        (Some(_), None) => Some(false),
        (None, Some(_)) => Some(true),
        both => panic!("two tooltips: {both:?}"),
    };

    click(cx, "fetch");
    finish(&app, cx, "the compact fetch", |cx| {
        confirms(cx, "fetch", "fetched") == Some(true)
    });
    assert_eq!(bounds(cx, "fetch"), square);
    assert!(cx.debug_bounds("fetch/fetched/text").is_none(), "no word");
    assert_eq!(name().as_deref(), Some("Fetch: Fetch from origin"));
    assert_eq!(announced(cx).unwrap().1, "Fetched origin");
    cx.simulate_mouse_move(point(px(4.), px(4.)), None, Modifiers::default());
    cx.simulate_mouse_move(square.center(), None, Modifiers::default());
    wait(cx, Duration::from_millis(600));
    assert_eq!(tooltip(cx), Some(true), "Fetched while it lasts");
    wait(cx, RESTORE_AFTER - Duration::from_millis(600));
    assert_eq!(confirms(cx, "fetch", "fetched"), Some(false));
    assert_eq!(tooltip(cx), Some(false), "its own words again");
    assert_eq!(name().as_deref(), Some("Fetch: Fetch from origin"));
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// Targets' Create reads Created where it was clicked once the branch is
/// created and switched to, though its name field empties, and Switch reads
/// Switched; each is announced once without the window-wide notice. A switch
/// chosen from the branch picker's menu confirms on the picker once it names
/// that branch: its check takes the branch icon's place.
#[gpui::test]
async fn create_switch_and_the_pickers_switch_confirm_where_they_were_asked(
    cx: &mut TestAppContext,
) {
    let (fixture, app, cx) = history_window(cx).await;
    fixture_git(
        &fixture.path().join("narrow"),
        &["reset", "--hard", "--quiet"],
    );
    let repository = app.read_with(cx, |app, _| app.path.clone());
    let name_branch = |cx: &mut VisualTestContext, name: &str| {
        let name = name.to_owned();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.git_actions_open = true;
                app.branch_name
                    .update(cx, |input, cx| input.set_value(name, window, cx));
                cx.notify();
            })
        });
        draw(cx);
    };
    for (id, state, branch, outcome) in [
        (
            "create-branch",
            "created",
            "feature",
            "Created and switched to feature",
        ),
        ("checkout-branch", "switched", "main", "Switched to main"),
    ] {
        name_branch(cx, branch);
        let rest = bounds(cx, id);
        assert_eq!(confirms(cx, id, state), Some(false), "{id}");
        click(cx, id);
        finish(&app, cx, id, |cx| confirms(cx, id, state) == Some(true));
        assert_eq!(bounds(cx, id), rest, "{id} keeps its bounds");
        assert_eq!(announced(cx).unwrap().1, outcome);
        app.read_with(cx, |app, cx| {
            assert_eq!(app.operation_notice, None);
            assert_eq!(app.branch_name.read(cx).value(), "", "the field emptied");
        });
        wait(cx, RESTORE_AFTER);
        assert_eq!(confirms(cx, id, state), Some(false), "{id}");
    }

    let icon = || {
        crate::views::CONTROL_ICONS
            .with(|icons| icons.borrow().get("current-branch").cloned())
            .unwrap()
    };
    assert_eq!(icon(), "icons/branch.svg");
    let picker = confirmations::picker(&repository, "feature");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.write_from(
                gitturtle_core::WriteCommand::Checkout {
                    branch: "feature".into(),
                },
                "Switching branch…",
                Some(picker),
                window,
                cx,
            )
        })
    });
    finish(&app, cx, "the picker's switch", |_| {
        icon() == "icons/check.svg"
    });
    let during = bounds(cx, "current-branch");
    assert_eq!(announced(cx).unwrap().1, "Switched to feature");
    wait(cx, RESTORE_AFTER);
    assert_eq!(icon(), "icons/branch.svg");
    assert_eq!(
        bounds(cx, "current-branch"),
        during,
        "only its icon changed"
    );
}

/// The status line keeps a write's outcome for as long as the control that
/// asked confirms it, over the refresh that follows the write: once a row's
/// Stage has read the working tree again, which writes Changes' working
/// summary to the status, the line still reads "Staged notes.txt", and
/// 1.5 s later it reads the summary. A Commit, which no control confirms
/// and whose quiet refresh writes the summary too, keeps its outcome the
/// same time. Before this, the refresh's summary replaced the outcome at
/// once.
#[gpui::test]
async fn the_status_line_keeps_a_writes_outcome_over_the_refresh_after_it(cx: &mut TestAppContext) {
    let (fixture, app, cx, repository) = working(cx).await;
    let line =
        |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.status_line().to_owned());
    assert_eq!(line(cx), "2 working changes");
    let notes = |area| {
        confirmations::file(&repository, Path::new("notes.txt"), area)
            .control()
            .to_string()
    };
    let (unstaged, staged) = (notes(ChangeArea::Unstaged), notes(ChangeArea::Staged));

    let rest = bounds(cx, &format!("{unstaged}/rest"));
    cx.simulate_click(rest.center(), Modifiers::default());
    // `finish` waits until the write and the status read after it are done.
    finish(&app, cx, "notes.txt arriving in Staged", |cx| {
        confirms(cx, &staged, "staged").is_some()
    });
    app.read_with(cx, |app, _| {
        assert!(
            app.status.starts_with("2 working changes · 1 staged"),
            "the refresh reported the working tree: {}",
            app.status
        )
    });
    assert_eq!(line(cx), "Staged notes.txt", "the refresh replaced it");
    wait(cx, RESTORE_AFTER - Duration::from_millis(1));
    assert_eq!(confirms(cx, &staged, "staged"), Some(true));
    assert_eq!(line(cx), "Staged notes.txt");
    wait(cx, Duration::from_millis(1));
    assert_eq!(confirms(cx, &staged, "staged"), Some(false));
    assert!(
        line(cx).starts_with("2 working changes · 1 staged"),
        "{}",
        line(cx)
    );

    let worktree = fixture.path().join("narrow");
    fixture_git(&worktree, &["config", "user.name", "Fixture"]);
    fixture_git(
        &worktree,
        &["config", "user.email", "fixture@example.invalid"],
    );
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
    // The commit's quiet refresh reads the one change left.
    finish(&app, cx, "the commit's quiet refresh", |cx| {
        app.read_with(cx, |app, _| app.status == "1 working change")
    });
    let committed = line(cx);
    assert!(
        committed.starts_with("Committed ") && committed.ends_with(" · Record notes"),
        "{committed}"
    );
    wait(cx, RESTORE_AFTER);
    assert_eq!(line(cx), "1 working change");
}
