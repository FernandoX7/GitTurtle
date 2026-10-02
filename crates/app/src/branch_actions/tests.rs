use super::*;
use crate::focus_reveal::tests::{assert_steady, assert_tab_reveals, control, focus_filter};
use crate::tags::tests::{
    assert_rings_whole, assert_room_moves_nothing, draw, git, rendered, scroll_down,
    tagged_repository, window,
};
use ::core::prelude::v1::test;

/// The branch chooser's list scrolls, so GPUI clips it to its bounds on both
/// axes, and it ends the dialog's body, which the dialog clips. Scrolled to
/// either end, the row at that end keeps its ring whole, and nothing moves.
#[gpui::test]
async fn branch_chooser_keeps_room_for_rings_at_either_end(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    for index in 0..11 {
        git(repo.path(), &["branch", &format!("feature-{index:02}")]);
    }
    let (app, cx) = window(cx, &repo);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.choose_branch(ChoicePurpose::Manage, window, cx)
        })
    });
    let read = app.update(cx, |app, _| app.branch_actions.task.take());
    read.expect("the branches are read").await;
    draw(cx);

    // Main and eleven features, in the order the list shows them.
    let (first, last) = ("branch-choice-0", "branch-choice-11");
    let fixed = ["branch-chooser-detail"];
    let list = rendered(cx, "branch-chooser-list");
    assert!(
        rendered(cx, last).bottom() > list.bottom(),
        "the list is long enough to scroll"
    );
    assert_rings_whole(cx, &[first]);
    assert_room_moves_nothing(cx, &app, &[first], &fixed);

    scroll_down(
        cx,
        list.center(),
        ScrollDelta::Pixels(point(px(0.), px(-10_000.))),
    );
    assert!(
        rendered(cx, first).top() < list.top(),
        "the list scrolls to its end"
    );
    assert_rings_whole(cx, &[last]);
    assert_room_moves_nothing(cx, &app, &[last], &fixed);
}

/// The application in a 1000 × 680 window on a repository with three tagged
/// commits, its branch `main` and `branches` more.
fn small_window_with_branches<'a>(
    cx: &'a mut TestAppContext,
    fixture: &std::path::Path,
    branches: usize,
) -> (GitRepository, Entity<GitTurtle>, &'a mut VisualTestContext) {
    let repo = tagged_repository(fixture);
    for index in 0..branches {
        git(repo.path(), &["branch", &format!("feature-{index:02}")]);
    }
    let (app, cx) = window(cx, &repo);
    cx.simulate_resize(size(px(1000.), px(680.)));
    draw(cx);
    (repo, app, cx)
}

/// Tab from the branch chooser's filter moves through every row and Shift+Tab
/// back, and the list scrolls each row into view with its focus ring. Once
/// a row is shown, redraws and a click on a partly visible row scroll
/// nothing.
#[gpui::test]
async fn tab_reveals_every_branch_chooser_row(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (_repo, app, cx) = small_window_with_branches(cx, fixture.path(), 29);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.choose_branch(ChoicePurpose::Manage, window, cx)
        })
    });
    let read = app.update(cx, |app, _| app.branch_actions.task.take());
    read.expect("the branches are read").await;
    draw(cx);
    let list = app
        .read_with(cx, |app, _| app.branch_actions.list.clone())
        .expect("the chooser's list");

    // Main and 29 features, in the order the list shows them.
    let rows: Vec<_> = (0..30)
        .map(|index| control("branch-choice", index))
        .collect();
    focus_filter(cx, &list, &rows[0]);
    assert_tab_reveals(cx, &list, "branch-chooser-list", &rows);
    // A click activates nothing while an operation runs.
    app.update(cx, |app, _| app.operation_busy = Some("Testing"));
    assert_steady(cx, &list, "branch-chooser-list", &rows);
}

/// Tab moves through every remote's Edit… and Remove… and Shift+Tab back,
/// and the remote manager's list scrolls each into view with its ring.
#[gpui::test]
async fn tab_reveals_every_remote_manager_control(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (repo, app, cx) = small_window_with_branches(cx, fixture.path(), 0);
    // Local bare remotes: the manager only lists them.
    let remotes = 12;
    for index in 0..remotes {
        let bare = fixture.path().join(format!("remote-{index:02}.git"));
        git(
            fixture.path(),
            &["init", "--quiet", "--bare", bare.to_str().unwrap()],
        );
        git(
            repo.path(),
            &[
                "remote",
                "add",
                &format!("remote-{index:02}"),
                bare.to_str().unwrap(),
            ],
        );
    }
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_remote_manager(window, cx)));
    let read = app.update(cx, |app, _| app.branch_actions.task.take());
    read.expect("the remotes are read").await;
    draw(cx);
    let list = app
        .read_with(cx, |app, _| app.branch_actions.list.clone())
        .expect("the manager's list");

    let controls: Vec<_> = (0..remotes)
        .flat_map(|index| ["edit-remote", "remove-remote"].map(|name| control(name, index)))
        .collect();
    focus_filter(cx, &list, &controls[0]);
    assert_tab_reveals(cx, &list, "remote-manager-list", &controls);
    app.update(cx, |app, _| app.operation_busy = Some("Testing"));
    assert_steady(cx, &list, "remote-manager-list", &controls);
}
