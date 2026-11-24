use super::*;
use crate::tags::tests::{draw, git, rendered, window};
use core::prelude::v1::test;
use std::fs;
use std::path::Path;

fn linear_rebase_repo(dir: &Path, commits: usize) -> GitRepository {
    let repo = crate::views::tests::fixture_repository(dir.join("rebase-drag"));
    fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "--quiet", "-m", "Base commit"]);
    git(repo.path(), &["tag", "base"]);
    for index in 1..=commits {
        let name = format!("f{index}.txt");
        fs::write(repo.path().join(&name), format!("line {index}\n")).unwrap();
        git(repo.path(), &["add", &name]);
        git(
            repo.path(),
            &["commit", "--quiet", "-m", &format!("Commit {index}")],
        );
    }
    repo
}

async fn open_loaded_rebase<'a>(
    cx: &'a mut TestAppContext,
    repo: &GitRepository,
    reduce_motion: bool,
) -> (
    Entity<GitTurtle>,
    Entity<RebaseForm>,
    &'a mut VisualTestContext,
) {
    let (app, cx) = window(cx, repo);
    cx.update(|_, cx| {
        cx.set_reduce_motion(reduce_motion);
        shortcuts::bind_keys(cx);
        init(cx);
        native_accessibility::bind_keys(cx);
    });
    cx.simulate_resize(size(px(1100.), px(780.)));
    app.update(cx, |app, cx| {
        app.work_status = Some(Arc::new(gitturtle_core::RepositoryStatus {
            branch: Some("main".into()),
            ..Default::default()
        }));
        cx.notify();
    });
    draw(cx);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_interactive_rebase(Some("base".into()), window, cx)
        });
    });
    if !reduce_motion {
        cx.executor()
            .advance_clock(*gpui_kit::component::dialog::ANIMATION_DURATION);
    }
    draw(cx);
    let form = app.read_with(cx, |app, _| {
        app.interactive_rebase
            .draft
            .clone()
            .expect("interactive rebase draft form")
    });
    let base = form.read_with(cx, |form, _| form.base.clone());
    cx.update(|window, cx| {
        base.update(cx, |input, cx| input.set_value("base", window, cx));
    });
    draw(cx);
    let load = rendered(cx, "load-rebase-commits");
    cx.simulate_click(load.center(), Modifiers::default());
    let read = app.update(cx, |app, _| app.interactive_rebase.task.take());
    if let Some(read) = read {
        read.await;
    }
    draw(cx);
    (app, form, cx)
}

fn step_oids(form: &Entity<RebaseForm>, cx: &VisualTestContext) -> Vec<String> {
    form.read_with(cx, |form, _| {
        form.steps.iter().map(|step| step.oid.clone()).collect()
    })
}

#[test]
fn reordered_steps_unit_covers_up_down_edges_and_noops() {
    let steps = vec![
        RebaseStep {
            oid: "a".into(),
            action: RebaseAction::Pick,
        },
        RebaseStep {
            oid: "b".into(),
            action: RebaseAction::Pick,
        },
        RebaseStep {
            oid: "c".into(),
            action: RebaseAction::Pick,
        },
    ];
    let oids = |v: Vec<RebaseStep>| v.into_iter().map(|s| s.oid).collect::<Vec<_>>();

    // No-op slots (before/after self) and out-of-bounds.
    assert_eq!(reordered_steps(&steps, 0, 0), None);
    assert_eq!(reordered_steps(&steps, 0, 1), None);
    assert_eq!(reordered_steps(&steps, 1, 1), None);
    assert_eq!(reordered_steps(&steps, 1, 2), None);
    assert_eq!(reordered_steps(&steps, 3, 0), None);
    assert_eq!(reordered_steps(&steps, 0, 4), None);

    // Move down to middle and bottom.
    let (next, dest) = reordered_steps(&steps, 0, 2).unwrap();
    assert_eq!(dest, 1);
    assert_eq!(oids(next), vec!["b", "a", "c"]);

    let (next, dest) = reordered_steps(&steps, 0, 3).unwrap();
    assert_eq!(dest, 2);
    assert_eq!(oids(next), vec!["b", "c", "a"]);

    // Move up to middle and top.
    let (next, dest) = reordered_steps(&steps, 2, 1).unwrap();
    assert_eq!(dest, 1);
    assert_eq!(oids(next), vec!["a", "c", "b"]);

    let (next, dest) = reordered_steps(&steps, 2, 0).unwrap();
    assert_eq!(dest, 0);
    assert_eq!(oids(next), vec!["c", "a", "b"]);
}

#[gpui::test]
async fn drag_reorder_up_and_down_with_drop_indicator_and_cancel_escape(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let repo = linear_rebase_repo(dir.path(), 4);
    let (_app, form, cx) = open_loaded_rebase(cx, &repo, true).await;

    let initial = step_oids(&form, cx);
    assert_eq!(initial.len(), 4);

    // Drag row 0 down to the lower half of row 2 (slot 3 -> dest index 2).
    let row0 = rendered(cx, "rebase-commit-0");
    let row2 = rendered(cx, "rebase-commit-2");
    let start = row0.center();
    let target_down = point(row2.center().x, row2.bottom() - px(4.));

    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(start.x, start.y + px(12.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_move(target_down, Some(MouseButton::Left), Modifiers::default());
    draw(cx);

    assert!(cx.update(|_, cx| cx.has_active_drag()), "drag is active");
    assert!(
        cx.debug_bounds("rebase-drop-indicator").is_some(),
        "drop indicator paints during active drag"
    );

    cx.simulate_mouse_up(target_down, MouseButton::Left, Modifiers::default());
    draw(cx);

    assert_eq!(
        step_oids(&form, cx),
        vec![
            initial[1].clone(),
            initial[2].clone(),
            initial[0].clone(),
            initial[3].clone()
        ]
    );
    form.read_with(cx, |form, _| {
        assert_eq!(form.selected, 2);
        assert!(form.error.is_none());
        assert!(form.drag_target.is_none());
    });
    assert!(cx.debug_bounds("rebase-drop-indicator").is_none());

    // Drag row 2 back up to the upper half of row 0 (slot 0 -> dest index 0).
    let row2 = rendered(cx, "rebase-commit-2");
    let row0 = rendered(cx, "rebase-commit-0");
    let start_up = row2.center();
    let target_up = point(row0.center().x, row0.top() + px(4.));

    cx.simulate_mouse_down(start_up, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(start_up.x, start_up.y - px(12.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_move(target_up, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    assert!(cx.debug_bounds("rebase-drop-indicator").is_some());
    cx.simulate_mouse_up(target_up, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert_eq!(step_oids(&form, cx), initial);

    // Start dragging row 0 toward row 3, then press Escape mid-drag:
    // must cancel drag without mutating steps or closing the dialog.
    let row0 = rendered(cx, "rebase-commit-0");
    let row3 = rendered(cx, "rebase-commit-3");
    let target_cancel = point(row3.center().x, row3.bottom() - px(4.));
    cx.simulate_mouse_down(row0.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(row0.center().x, row0.center().y + px(12.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_move(target_cancel, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    assert!(cx.update(|_, cx| cx.has_active_drag()));
    assert!(cx.debug_bounds("rebase-drop-indicator").is_some());

    cx.simulate_keystrokes("escape");
    draw(cx);

    assert!(
        !cx.update(|_, cx| cx.has_active_drag()),
        "Escape cancels active drag"
    );
    form.read_with(cx, |form, _| {
        assert!(!form.closed, "Escape mid-drag must not close dialog");
        assert!(form.drag_target.is_none());
    });
    assert_eq!(step_oids(&form, cx), initial, "Escape preserves step order");
    assert!(cx.debug_bounds("rebase-drop-indicator").is_none());
    cx.simulate_mouse_up(target_cancel, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert_eq!(step_oids(&form, cx), initial);
}

#[gpui::test]
async fn drag_enforces_rebase_invariants_live_for_squash_and_fixup(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let repo = linear_rebase_repo(dir.path(), 3);
    let (_app, form, cx) = open_loaded_rebase(cx, &repo, true).await;
    let initial = step_oids(&form, cx);

    // Mark row 1 as Squash: [Pick 0, Squash 1, Pick 2].
    form.update(cx, |form, cx| {
        form.selected = 1;
        form.choose(RebaseAction::Squash, cx);
    });
    draw(cx);

    // Dragging Squash (row 1) to position 0 (top of row 0) is rejected cleanly.
    let row1 = rendered(cx, "rebase-commit-1");
    let row0 = rendered(cx, "rebase-commit-0");
    let top_of_row0 = point(row0.center().x, row0.top() + px(4.));
    native_accessibility::written::forget();
    cx.simulate_mouse_down(row1.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(row1.center().x, row1.center().y - px(10.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_move(top_of_row0, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(top_of_row0, MouseButton::Left, Modifiers::default());
    draw(cx);

    assert_eq!(
        step_oids(&form, cx),
        initial,
        "invalid drop of Squash to slot 0 preserves valid order"
    );
    form.read_with(cx, |form, _| {
        assert!(
            form.error.is_some(),
            "rejected drop surfaces an inline validation notice"
        );
    });
    assert!(
        cx.debug_bounds("rebase-form-error").is_some(),
        "inline error is rendered"
    );
    assert!(
        native_accessibility::written::get("rebase-form-error").is_some(),
        "inline error is recorded in accessibility tree"
    );
    let alert = crate::copy_feedback::tests::alerted(cx)
        .expect("rejected drop posts an assertive accessibility alert");
    assert!(
        alert.1.to_lowercase().contains("squash"),
        "unexpected alert text: {}",
        alert.1
    );

    // Dragging the sole preceding Pick (row 0) past Fixup (row 1) is also rejected.
    form.update(cx, |form, cx| {
        form.selected = 1;
        form.choose(RebaseAction::Fixup, cx);
    });
    draw(cx);
    let row0 = rendered(cx, "rebase-commit-0");
    let row2 = rendered(cx, "rebase-commit-2");
    let bottom_of_row2 = point(row2.center().x, row2.bottom() - px(4.));
    cx.simulate_mouse_down(row0.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(row0.center().x, row0.center().y + px(12.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_move(
        bottom_of_row2,
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(bottom_of_row2, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert_eq!(step_oids(&form, cx), initial);
    assert!(form.read_with(cx, |form, _| form.error.is_some()));

    // Conversely, if row 0 was set to Squash via keyboard (currently invalid),
    // dragging a Pick row above it fixes the plan cleanly!
    form.update(cx, |form, cx| {
        form.selected = 1;
        form.choose(RebaseAction::Pick, cx);
        form.selected = 0;
        form.choose(RebaseAction::Squash, cx);
    });
    draw(cx);
    let row1 = rendered(cx, "rebase-commit-1");
    let row0 = rendered(cx, "rebase-commit-0");
    let top_of_row0 = point(row0.center().x, row0.top() + px(4.));
    cx.simulate_mouse_down(row1.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(row1.center().x, row1.center().y - px(10.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    cx.simulate_mouse_move(top_of_row0, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(top_of_row0, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert_eq!(
        step_oids(&form, cx),
        vec![initial[1].clone(), initial[0].clone(), initial[2].clone()]
    );
    form.read_with(cx, |form, cx| {
        assert!(form.error.is_none());
        assert!(form.notices(cx).0.is_none(), "repaired plan is valid");
    });
}

#[gpui::test]
async fn drag_respects_reduced_motion_and_keyboard_alt_arrows_parity(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let repo = linear_rebase_repo(dir.path(), 3);
    // Test with reduce_motion = false (full motion) as well as keyboard parity.
    let (_app, form, cx) = open_loaded_rebase(cx, &repo, false).await;
    let initial = step_oids(&form, cx);

    // Keyboard Alt+Down / Alt+Up parity.
    cx.simulate_keystrokes("alt-down");
    draw(cx);
    assert_eq!(
        step_oids(&form, cx),
        vec![initial[1].clone(), initial[0].clone(), initial[2].clone()]
    );
    cx.simulate_keystrokes("alt-up");
    draw(cx);
    assert_eq!(step_oids(&form, cx), initial);

    // Drag with full motion works cleanly too, including a fast single
    // coalesced pointer move directly to the drop target without an
    // intermediate drag_move event.
    let row0 = rendered(cx, "rebase-commit-0");
    let row2 = rendered(cx, "rebase-commit-2");
    let bottom = point(row2.center().x, row2.bottom() - px(4.));
    cx.simulate_mouse_down(row0.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(bottom, Some(MouseButton::Left), Modifiers::default());
    draw(cx);
    cx.simulate_mouse_up(bottom, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert_eq!(
        step_oids(&form, cx),
        vec![initial[1].clone(), initial[2].clone(), initial[0].clone()]
    );
}

#[gpui::test]
async fn drag_auto_scrolls_scrollable_sequence_near_edges(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let repo = linear_rebase_repo(dir.path(), 14);
    let (_app, form, cx) = open_loaded_rebase(cx, &repo, true).await;

    let list_bounds = rendered(cx, "rebase-list-focus");
    let row0 = rendered(cx, "rebase-commit-0");
    let near_bottom = point(list_bounds.center().x, list_bounds.bottom() - px(6.));

    cx.simulate_mouse_down(row0.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(row0.center().x, row0.center().y + px(12.)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    for step in 0..4 {
        let p = point(near_bottom.x + px(step as f32), near_bottom.y);
        cx.simulate_mouse_move(p, Some(MouseButton::Left), Modifiers::default());
        draw(cx);
    }

    let scrolled_down = form.read_with(cx, |form, _| form.scroll.0.borrow().base_handle.offset().y);
    assert!(
        scrolled_down < px(0.),
        "dragging near the bottom edge should auto-scroll down, got {scrolled_down:?}"
    );
    assert_eq!(
        form.read_with(cx, |form, _| form.rest.get()),
        scrolled_down,
        "wheel-chain rest tracks auto-scroll"
    );

    // Drag back near the top edge and verify auto-scroll moves upward.
    let near_top = point(list_bounds.center().x, list_bounds.top() + px(6.));
    for step in 0..4 {
        let p = point(near_top.x + px(step as f32), near_top.y);
        cx.simulate_mouse_move(p, Some(MouseButton::Left), Modifiers::default());
        draw(cx);
    }
    let scrolled_up = form.read_with(cx, |form, _| form.scroll.0.borrow().base_handle.offset().y);
    assert!(
        scrolled_up > scrolled_down,
        "dragging near the top edge should auto-scroll back up: {scrolled_up:?} vs {scrolled_down:?}"
    );

    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.simulate_mouse_up(near_top, MouseButton::Left, Modifiers::default());
    draw(cx);
}
