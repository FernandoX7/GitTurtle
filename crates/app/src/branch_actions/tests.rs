use super::*;
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
