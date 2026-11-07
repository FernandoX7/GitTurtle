use super::*;
use crate::focus_reveal::tests::{
    assert_every_frame_reveals, assert_steady, assert_tab_reveals, control, focus_filter,
};
use crate::tags::tests::{
    add_remotes, assert_no_scroll_cue, assert_rings_whole, assert_room_moves_nothing,
    assert_scroll_cue, draw, git, rendered, scroll_down, selectors, tagged_repository, window,
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

/// Tab and Shift+Tab through the branch chooser's 30 rows: every frame
/// painted after a key, the first one that draws the new focus included,
/// shows the focused row with its whole ring inside the list.
#[gpui::test]
async fn branch_chooser_reveals_rows_in_the_frame_that_draws_focus(cx: &mut TestAppContext) {
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

    let rows: Vec<_> = (0..30)
        .map(|index| control("branch-choice", index))
        .collect();
    focus_filter(cx, &list, &rows[0]);
    assert_every_frame_reveals(cx, &list, "branch-chooser-list", &rows);
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

/// The branch chooser of `main` and `branches` more open at 1000 × 680, and
/// its list.
async fn open_branch_chooser<'a>(
    cx: &'a mut TestAppContext,
    fixture: &std::path::Path,
    branches: usize,
) -> (Entity<GitTurtle>, FocusReveal, &'a mut VisualTestContext) {
    let (_repo, app, cx) = small_window_with_branches(cx, fixture, branches);
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
    (app, list, cx)
}

/// The remote manager of `remotes` remotes open at 1000 × 680, and its list.
async fn open_remote_manager<'a>(
    cx: &'a mut TestAppContext,
    fixture: &std::path::Path,
    remotes: usize,
) -> (Entity<GitTurtle>, FocusReveal, &'a mut VisualTestContext) {
    let (repo, app, cx) = small_window_with_branches(cx, fixture, 0);
    add_remotes(fixture, &repo, remotes);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_remote_manager(window, cx)));
    let read = app.update(cx, |app, _| app.branch_actions.task.take());
    read.expect("the remotes are read").await;
    draw(cx);
    let list = app
        .read_with(cx, |app, _| app.branch_actions.list.clone())
        .expect("the manager's list");
    (app, list, cx)
}

/// The branch chooser's 30 rows overflow its list at 1000 × 680, and its
/// scrollbar's thumb follows the list's offset in a gutter clear of every
/// ring.
#[gpui::test]
async fn branch_chooser_cues_rows_beyond_its_edge(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (app, list, cx) = open_branch_chooser(cx, fixture.path(), 29).await;
    let room = cx.update(|_, cx| appearance::button_ring_room(cx));
    let rows = selectors("branch-choice", 30);
    assert_scroll_cue(cx, &app, list.scroll(), "branch-chooser-list", room, &rows);
}

/// The branch chooser's three rows fit its list: it paints no thumb, and its
/// rows keep their width.
#[gpui::test]
async fn branch_chooser_that_fits_paints_no_thumb(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (app, _list, cx) = open_branch_chooser(cx, fixture.path(), 2).await;
    let room = cx.update(|_, cx| appearance::button_ring_room(cx));
    assert_no_scroll_cue(
        cx,
        &app,
        "branch-chooser-list",
        room,
        &selectors("branch-choice", 3),
    );
}

/// Twelve remotes overflow the remote manager's list at 1000 × 680, and its
/// scrollbar's thumb follows the list's offset in a gutter clear of every
/// remote and its ring room.
#[gpui::test]
async fn remote_manager_cues_remotes_beyond_its_edge(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (app, list, cx) = open_remote_manager(cx, fixture.path(), 12).await;
    let controls = [
        selectors("managed-remote", 12),
        selectors("edit-remote", 12),
        selectors("remove-remote", 12),
    ]
    .concat();
    // Each remote keeps its controls' ring room inside it; the list keeps
    // none around the remotes.
    assert_scroll_cue(
        cx,
        &app,
        list.scroll(),
        "remote-manager-list",
        px(0.),
        &controls,
    );
}

/// Two remotes fit the remote manager's list: it paints no thumb, and each
/// remote keeps the list's full width.
#[gpui::test]
async fn remote_manager_that_fits_paints_no_thumb(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (app, _list, cx) = open_remote_manager(cx, fixture.path(), 2).await;
    assert_no_scroll_cue(
        cx,
        &app,
        "remote-manager-list",
        px(0.),
        &selectors("managed-remote", 2),
    );
}

/// Asks for the open branch or remote form's review, as its button does,
/// lets its preparation fail and returns the node that announces why: the
/// form opens again with the reason, which holds `reason`, in its line
/// beneath the fields, `line`, an alert announced at once.
async fn review_fails(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    line: &str,
    reason: &str,
) -> String {
    cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
            cx,
        )
    });
    let read = app.update(cx, |app, _| app.branch_actions.task.take());
    read.expect("the review is prepared").await;
    draw(cx);
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "{line}: the form opens again"
    );
    assert!(
        cx.debug_bounds(line.to_owned().leak()).is_some(),
        "{line}: the form draws why"
    );
    let mut announced = native_accessibility::written::announced_from(&format!("{line}-"));
    assert_eq!(
        announced.len(),
        1,
        "{line}: one line announced: {announced:?}"
    );
    let (node, how) = announced.pop().unwrap();
    assert_eq!(
        how,
        native_accessibility::written::Announced::ASSERTIVE,
        "{line}: announced at once"
    );
    // The reason is its name, which AT-SPI announces, and its value, which
    // macOS does.
    let written = native_accessibility::written::get(&node).expect("the line's node");
    let (name, value) = (written.node.label(), written.node.value());
    assert!(
        name.is_some_and(|name| name.contains(reason)) && value == name,
        "{line}: named and valued by why: {name:?}, {value:?}"
    );
    node
}

/// A branch or remote form whose review cannot be prepared opens again with
/// the reason beneath its fields, an alert announced at once, as Rename
/// branch… does for the branch's own name and Edit remote for a remote
/// removed meanwhile. Opened again with that line already there, after
/// another review fails the same way, the line is a fresh node, announced
/// again as when it first appeared. The line was no live region, so neither
/// was announced; and a form that closes and opens again within a frame, as
/// a quick preparation can, keeps its nodes, which assistive technology
/// does not announce again.
#[gpui::test]
async fn a_reopened_branch_or_remote_form_announces_why_afresh(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let (repo, app, cx) = small_window_with_branches(cx, fixture.path(), 1);
    // Rename branch… starts with the branch's own name, which the core
    // refuses.
    let plan = Arc::new(repo.branch_plan("feature-00").unwrap());
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_branch_form(BranchFormMode::Rename(plan), window, cx)
        })
    });
    draw(cx);
    let named = "is already named 'feature-00'";
    let first = review_fails(&app, cx, "branch-form-error", named).await;
    let second = review_fails(&app, cx, "branch-form-error", named).await;
    assert_ne!(
        first, second,
        "Rename branch…: a fresh node announces it again"
    );
    cx.update(|window, cx| window.close_dialog(cx));
    draw(cx);

    // Edit remote for a remote removed after the form opened.
    let bare = fixture.path().join("upstream.git");
    git(
        fixture.path(),
        &["init", "--quiet", "--bare", bare.to_str().unwrap()],
    );
    git(
        repo.path(),
        &["remote", "add", "upstream", bare.to_str().unwrap()],
    );
    let expected = repo
        .remote_configs()
        .unwrap()
        .into_iter()
        .find(|remote| remote.name == "upstream")
        .expect("the remote");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_remote_form(Some(Arc::new(expected)), window, cx)
        })
    });
    draw(cx);
    git(repo.path(), &["remote", "remove", "upstream"]);
    let gone = "This remote no longer exists";
    let first = review_fails(&app, cx, "remote-form-error", gone).await;
    let second = review_fails(&app, cx, "remote-form-error", gone).await;
    assert_ne!(
        first, second,
        "Edit remote: a fresh node announces it again"
    );
}

/// A branch preparation that has ended no longer runs, so Changes opened
/// after it drops no plan: a plan is dropped only while its preparation runs
/// (`dismiss_branch_action`), as a recovery's is. The ended task stayed, and
/// each later Changes marked a plan dropped that nothing was preparing.
#[gpui::test]
async fn changes_after_a_branch_preparation_ends_drops_no_plan(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let repo = tagged_repository(fixture.path());
    let (app, cx) = window(cx, &repo);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.choose_branch(ChoicePurpose::Manage, window, cx)
        })
    });
    assert_eq!(
        app.read_with(cx, |app, _| app.operation_busy),
        Some(PREPARING)
    );
    app.read_with(cx, |app, _| app.operations.drain());
    crate::views::tests::settle(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert_eq!(app.operation_busy, None, "the preparation ended");
        assert!(app.branch_actions.task.is_none(), "it no longer runs");
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    assert!(
        !app.read_with(cx, |app, _| app.branch_actions.dismissed),
        "Changes drops no plan"
    );
}

/// History of three commits on `main` with the menu keys bound, a branch
/// `review` checked out in a linked worktree, read again as Refresh reads
/// it so the app knows that worktree, and the repository's path.
async fn with_held_branch(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    &mut VisualTestContext,
    std::path::PathBuf,
) {
    use crate::views::tests::{fixture_git, history_script};
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, history_script(3, 1)).await;
    cx.update(|_, cx| {
        crate::shortcuts::bind_keys(cx);
        crate::native_accessibility::bind_keys(cx);
    });
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    fixture_git(&repository, &["branch", "review"]);
    let linked = fixture.path().join("linked");
    fixture_git(
        &repository,
        &["worktree", "add", "-q", linked.to_str().unwrap(), "review"],
    );
    reopen(&app, cx, &repository).await;
    (fixture, app, cx, repository)
}

/// Reads `repository` again as History's Refresh does.
async fn reopen(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, repository: &std::path::Path) {
    let repository = repository.to_owned();
    cx.update(|window, cx| app.update(cx, |app, cx| app.open(repository, None, window, cx)));
    crate::views::tests::settle(app, cx).await;
}

/// The branch picker's menu keeps a branch another worktree has checked
/// out in the keys' order, as every row that says why it is unavailable
/// (`native_accessibility::detailed_item`): drawn as the branch's name with
/// "Another worktree" beside it in secondary text and named "review,
/// another worktree" and the way to it, Down from Find or create a branch…
/// rests the cursor on the current branch, which says so, and the next Down
/// on it, drawn there; Enter there switches nothing and leaves the menu
/// open with the cursor on it; the next Down moves on to Current branch
/// settings…, and Up comes back. Decided by the coordinator, by the rule to
/// say why something is unavailable: drawn as "review · another worktree",
/// one disabled label, it was passed over, Down going from Find or create a
/// branch… straight to Current branch settings…, so a screen reader never
/// reached why the branch could not be switched to.
#[gpui::test]
async fn the_branch_pickers_keys_rest_on_a_branch_another_worktree_holds(cx: &mut TestAppContext) {
    use crate::views::tests::fixture_git_output;
    const FIND: &str = "menu-item: Find or create a branch… · MenuItem";
    const CURRENT: &str = "menu-item: main, current branch · MenuItemCheckBox True disabled";
    const HELD: &str = "menu-item: review, another worktree. Open that worktree from Worktrees… · MenuItemCheckBox False disabled";
    const SETTINGS: &str = "menu-item: Current branch settings… · MenuItem";
    let (_fixture, app, cx, repository) = with_held_branch(cx).await;
    let head = || {
        let output = fixture_git_output(&repository, &["rev-parse", "--abbrev-ref", "HEAD"]);
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    assert_eq!(head(), "main");

    let picker = rendered(cx, "current-branch");
    cx.simulate_click(picker.center(), gpui::Modifiers::default());
    draw(cx);
    let muted = cx.update(|_, cx| gpui_kit::component::Theme::global(cx).muted_foreground);
    let reason = format!(
        "menu-detail: Another worktree · {:?}",
        gpui::Rgba::from(muted)
    );
    assert!(
        cx.debug_bounds(reason.leak()).is_some(),
        "the reason is drawn beside the branch in secondary text"
    );
    let cursor = |cx: &mut VisualTestContext| {
        let fill = cx.update(|_, cx| {
            gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).list_active)
        });
        [FIND, CURRENT, HELD, SETTINGS]
            .into_iter()
            .filter(|item| {
                let bounds = cx
                    .debug_bounds(item)
                    .unwrap_or_else(|| panic!("{item} drawn"));
                crate::appearance::painted_row(cx, bounds).0.contains(&fill)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(cursor(cx), Vec::<&str>::new(), "a click opens no cursor");

    for (key, on) in [("down", FIND), ("down", CURRENT), ("down", HELD)] {
        cx.simulate_keystrokes(key);
        draw(cx);
        assert_eq!(cursor(cx), [on], "after {key}");
    }
    cx.simulate_keystrokes("enter");
    crate::views::tests::settle(&app, cx).await;
    assert!(
        app.read_with(cx, |app, _| app.operation_busy.is_none()
            && app.operation_task.is_none()),
        "Enter on the held branch starts no write"
    );
    assert_eq!(head(), "main", "Enter switches nothing");
    assert_eq!(cursor(cx), [HELD], "the menu stays open on the branch");

    for (key, on) in [
        ("down", SETTINGS),
        ("up", HELD),
        ("up", CURRENT),
        ("up", FIND),
    ] {
        cx.simulate_keystrokes(key);
        draw(cx);
        assert_eq!(cursor(cx), [on], "after {key}");
    }
    cx.simulate_keystrokes("escape");
    draw(cx);
}

/// The branch picker's open menu keeps keyboard focus, as a dialog does
/// (decided 2026-11-07): it is one stop, so Tab and Shift+Tab, pressed on
/// past its last and its first stop, leave focus on the menu and the keys'
/// cursor on its row, and the menu stays drawn; Escape closes it and gives
/// focus back to the picker that opened it. At 13 pt in a 461 × 490 half
/// tile QA-106e tabbed from the open menu to Main Menu, the tab and its
/// close button behind it while it stayed drawn; the same at 1480 × 973.
#[gpui::test]
async fn tab_and_shift_tab_stay_inside_the_open_branch_chooser(cx: &mut TestAppContext) {
    const FIND: &str = "menu-item: Find or create a branch… · MenuItem";
    const CURRENT: &str = "menu-item: main, current branch · MenuItemCheckBox True disabled";
    const HELD: &str = "menu-item: review, another worktree. Open that worktree from Worktrees… · MenuItemCheckBox False disabled";
    let (_fixture, _app, cx, _repository) = with_held_branch(cx).await;
    // The rows that draw the keys' cursor.
    let cursor = |cx: &mut VisualTestContext| {
        let fill = cx.update(|_, cx| {
            gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).list_active)
        });
        [FIND, CURRENT, HELD]
            .into_iter()
            .filter(|item| {
                cx.debug_bounds(item).is_some_and(|bounds| {
                    crate::appearance::painted_row(cx, bounds).0.contains(&fill)
                })
            })
            .collect::<Vec<_>>()
    };
    let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
    let press = |cx: &mut VisualTestContext, keys: &str| {
        cx.simulate_keystrokes(keys);
        draw(cx);
    };
    let drawn = |cx: &mut VisualTestContext| cx.debug_bounds(FIND).is_some();
    // Tabs on to the picker, as QA-106e reached it, and opens the chooser
    // with Space, returning the picker's focus: the menu button whose Space
    // draws the chooser. A press does not focus a Button.
    let open_from_the_picker = |cx: &mut VisualTestContext, at: &str| {
        for _ in 0..80 {
            let on_a_menu_button = cx.update(|window, _| {
                (window.context_stack().iter()).any(|context| context.contains("PopoverTrigger"))
            });
            if on_a_menu_button {
                let opener = focused(cx).expect("the menu button holds focus");
                press(cx, "space");
                if drawn(cx) {
                    return opener;
                }
                // Another button's menu: Escape gives focus back to it.
                if focused(cx).as_ref() != Some(&opener) {
                    press(cx, "escape");
                }
            }
            press(cx, "tab");
        }
        panic!("{at}: Tab never reached the branch picker");
    };
    for (width, height) in [(461., 490.), (1480., 973.)] {
        let at = format!("13 pt in {width} × {height}");
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        let opener = open_from_the_picker(cx, &at);
        let menu = focused(cx).unwrap_or_else(|| panic!("{at}: focus in the chooser"));
        assert_ne!(menu, opener, "{at}: the chooser takes focus");
        press(cx, "down");
        press(cx, "down");
        assert_eq!(cursor(cx), [HELD], "{at}: the keys rest on the held branch");
        for key in [
            "tab",
            "tab",
            "tab",
            "shift-tab",
            "shift-tab",
            "shift-tab",
            "shift-tab",
        ] {
            press(cx, key);
            assert_eq!(
                focused(cx).as_ref(),
                Some(&menu),
                "{at}: focus stays in the chooser after {key}"
            );
            assert!(drawn(cx), "{at}: the chooser stays drawn after {key}");
            assert_eq!(cursor(cx), [HELD], "{at}: {key} moves no cursor");
        }
        press(cx, "escape");
        assert!(!drawn(cx), "{at}: Escape closes the chooser");
        assert_eq!(
            focused(cx).as_ref(),
            Some(&opener),
            "{at}: Escape gives focus back to the picker"
        );
    }
}

/// In a 461 × 490 half tile, at 13 and 18 pt, Tab from the branch chooser's
/// filter through its 30 rows and Shift+Tab back shows each row whole with
/// its ring, inside every clip around it, the dialog's among them. Laid out
/// at full height in the dialog's scrolling body, the list ran below the
/// dialog's fold: a row inside the list's view but past the fold took focus
/// where nothing showed it.
#[gpui::test]
async fn a_half_tile_reveals_every_branch_chooser_row(cx: &mut TestAppContext) {
    use crate::tags::tests::{content_mask, holds, set_interface_text_size};
    let fixture = tempfile::tempdir().unwrap();
    let (app, list, cx) = open_branch_chooser(cx, fixture.path(), 29).await;
    let room = cx.update(|_, cx| appearance::button_ring_room(cx));
    let rows: Vec<_> = (0..30)
        .map(|index| control("branch-choice", index))
        .collect();
    let shown = |cx: &mut VisualTestContext, key: &str, (id, row): &(ElementId, &'static str)| {
        assert_eq!(
            cx.update(|window, _| list.focused(window)).as_ref(),
            Some(id),
            "{key} focuses {row}"
        );
        let mask = content_mask(cx, row);
        let ring = rendered(cx, row).dilate(room);
        assert!(
            holds(mask, ring),
            "after {key}, {row} and its ring {ring:?} lie outside what the dialog shows, {mask:?}"
        );
    };
    // The half tile at both sizes, then shorter windows at 18 pt, where the
    // detail line, the filter and the list's two-row floor come nearest the
    // dialog's cap.
    for (height, text) in [(490., 13), (490., 18), (400., 18), (360., 18)] {
        cx.simulate_resize(size(px(461.), px(height)));
        draw(cx);
        set_interface_text_size(cx, &app, text);
        let text = format!("{text} pt in 461 × {height}");
        focus_filter(cx, &list, &rows[0]);
        for row in &rows {
            cx.simulate_keystrokes("tab");
            draw(cx);
            shown(cx, &format!("tab at {text}"), row);
        }
        for row in rows.iter().rev().skip(1) {
            cx.simulate_keystrokes("shift-tab");
            draw(cx);
            shown(cx, &format!("shift-tab at {text}"), row);
        }
        // Back onto the filter, for the next size.
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
    }
    set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
}

/// Every item the branch picker's menu refuses says why, by the rule that
/// a nearby disabled reason is heard as well as seen: each is drawn with its
/// reason beside its command, named by both, and the menu keys rest on it
/// (`PopupMenuItem::reason_stated`). At rest that is the current branch,
/// "main, current branch", and a branch another worktree has checked out,
/// "review, another worktree" and the way to it; while a write runs, also
/// every item the write refuses, named by the write. The current branch was
/// ticked and refused with nothing to say why, so the keys passed over it
/// and a screen reader never heard which branch was current.
#[gpui::test]
async fn every_item_the_branch_picker_refuses_says_why(cx: &mut TestAppContext) {
    const CURRENT: &str = "main, current branch";
    const HELD: &str = "review, another worktree. Open that worktree from Worktrees…";
    let (_fixture, app, cx, _repository) = with_held_branch(cx).await;
    let open = |cx: &mut VisualTestContext| {
        PICKER_ITEMS.with(|items| items.borrow_mut().clear());
        let picker = rendered(cx, "current-branch");
        cx.simulate_click(picker.center(), gpui::Modifiers::default());
        draw(cx);
    };
    // Each refused item's accessible name, once it is drawn under it and
    // says more than its command.
    let refused = |cx: &mut VisualTestContext| -> Vec<String> {
        let items = PICKER_ITEMS.with(|items| items.borrow().clone());
        assert!(!items.is_empty(), "the menu is built");
        items
            .into_iter()
            .filter(|item| item.disabled)
            .map(|item| {
                let name = item.name.clone().unwrap_or_else(|| item.label.clone());
                assert!(
                    item.reason_stated && name.starts_with(&format!("{}, ", item.label)),
                    "{item:?} says why it is refused"
                );
                let (role, ticked) = match item.checked {
                    Some(true) => ("MenuItemCheckBox", " True"),
                    Some(false) => ("MenuItemCheckBox", " False"),
                    None => ("MenuItem", ""),
                };
                let drawn = format!("menu-item: {name} · {role}{ticked} disabled");
                assert!(cx.debug_bounds(drawn.clone().leak()).is_some(), "{drawn}");
                name
            })
            .collect()
    };

    open(cx);
    assert_eq!(refused(cx), [CURRENT, HELD]);
    let muted = cx.update(|_, cx| gpui_kit::component::Theme::global(cx).muted_foreground);
    let reason = format!(
        "menu-detail: Current branch · {:?}",
        gpui::Rgba::from(muted)
    );
    assert!(
        cx.debug_bounds(reason.leak()).is_some(),
        "the current branch's reason is drawn beside it in secondary text"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);

    app.update(cx, |app, cx| {
        app.operation_busy = Some("Fetching remote…");
        cx.notify();
    });
    draw(cx);
    open(cx);
    let during = refused(cx);
    let waiting = during
        .iter()
        .filter(|name| name.ends_with(", available when fetching remote finishes"))
        .count();
    assert!(
        during.contains(&CURRENT.to_owned()) && during.contains(&HELD.to_owned()) && waiting > 1,
        "the branches keep their own reasons, and the write's are named: {during:?}"
    );
    cx.simulate_keystrokes("escape");
    app.update(cx, |app, _| app.operation_busy = None);
    draw(cx);
}

/// The actions a branch's or a remote branch's dialog can draw.
const DIALOG_ACTIONS: [&str; 9] = [
    "rename-branch",
    "choose-upstream",
    "remove-upstream",
    "delete-branch",
    "merge-selected-branch",
    "rebase-selected-branch",
    "track-remote-branch",
    "merge-remote-branch",
    "rebase-remote-branch",
];

/// Closes the dialog open, if any, and forgets what it drew.
fn close_dialog(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.close_dialog(cx));
    draw(cx);
    native_accessibility::written::forget();
}

/// Opens local branch `name`'s dialog, as the branch picker and the chooser
/// do, in place of the one open.
async fn open_branch(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, name: &str) {
    close_dialog(cx);
    let name = name.to_owned();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_contextual_branch(name, false, window, cx)
        })
    });
    let read = app.update(cx, |app, _| app.branch_actions.task.take());
    read.expect("the branch is read").await;
}

/// Whether the pointer resting on action `id` shows the tooltip `text`, as
/// the window draws it under the pointer; the pointer then leaves the window
/// until the tooltip has gone.
fn hover_shows(cx: &mut VisualTestContext, id: &'static str, text: &str) -> bool {
    const SHOW_DELAY: std::time::Duration = std::time::Duration::from_millis(600);
    let action = rendered(cx, id);
    cx.simulate_mouse_move(action.center(), None, gpui::Modifiers::default());
    cx.executor().advance_clock(SHOW_DELAY);
    draw(cx);
    let shown = cx
        .debug_bounds(format!("{id}/tooltip/{text}").leak())
        .is_some();
    cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, gpui::Modifiers::default());
    cx.executor().advance_clock(SHOW_DELAY);
    draw(cx);
    shown
}

/// Each action the open dialog draws refused, with why, once the tooltip a
/// pointer resting on it shows says the same as its description; an action
/// it offers says neither.
fn refused_actions(cx: &mut VisualTestContext) -> Vec<(&'static str, String)> {
    use crate::tags::tests::hovered_tooltips;
    draw(cx);
    let drawn: Vec<_> = DIALOG_ACTIONS
        .into_iter()
        .filter_map(|id| Some((id, native_accessibility::written::get(id)?.node)))
        .collect();
    assert!(!drawn.is_empty(), "the dialog draws its actions");
    drawn
        .into_iter()
        .filter_map(|(id, node)| {
            let description = node.description().map(str::to_owned);
            if !node.is_disabled() {
                assert_eq!(description, None, "{id} is offered");
                assert_eq!(hovered_tooltips(cx, id), 0, "{id} is offered");
                return None;
            }
            let why = description.unwrap_or_else(|| panic!("{id} describes why"));
            assert!(hover_shows(cx, id, &why), "{id}: its tooltip says {why:?}");
            Some((id, why))
        })
        .collect()
}

/// Every action a branch's or a remote branch's dialog refuses says why,
/// and the way to it, in its tooltip and its accessible description, which
/// are the same words, as New theme… does at its bound; an action it offers
/// gives none. A branch another worktree has checked out is renamed and
/// tracked from there, its upstream, once it has one, removed from there,
/// and deleted once that worktree has switched away; a branch with no
/// upstream, held elsewhere or the current one, has none to remove; a
/// branch with a commit the current branch lacks keeps that work; a remote
/// branch merges or rebases only onto a branch. Those actions were dimmed
/// with nothing to say why, and the dialog's note named only the deletion's
/// reason.
#[gpui::test]
async fn every_action_a_branch_dialog_refuses_says_why(cx: &mut TestAppContext) {
    use crate::views::tests::{fixture_git, fixture_git_output};
    let (_fixture, app, cx, repository) = with_held_branch(cx).await;
    // The dialogs slide in on the real clock: at rest from their first
    // frame, each action lies where the pointer rests on it.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    // `ahead` holds a commit `main` lacks.
    let commit = fixture_git_output(
        &repository,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit-tree",
            "main^{tree}",
            "-p",
            "main",
            "-m",
            "ahead",
        ],
    );
    let ahead = String::from_utf8_lossy(&commit.stdout).trim().to_owned();
    fixture_git(&repository, &["branch", "ahead", &ahead]);
    reopen(&app, cx, &repository).await;
    let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
    let checked_out = |name: &str| {
        let plan = repo.branch_plan(name).unwrap();
        format!(
            "Checked out in {}. Switch that worktree to another branch before deleting.",
            path_list(&plan.checked_out_in)
        )
    };
    let remote = |cx: &mut VisualTestContext| {
        close_dialog(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let reference = "refs/remotes/origin/feature".to_owned();
                app.show_remote_branch(reference, ahead.clone(), window, cx)
            })
        });
    };

    let unset = "No upstream is set. Change upstream… sets one.".to_owned();
    let elsewhere = "Checked out in another worktree.";
    let held = |cleared: String| {
        [
            (
                "rename-branch",
                format!("{elsewhere} Rename it from that worktree."),
            ),
            (
                "choose-upstream",
                format!("{elsewhere} Change its upstream from that worktree."),
            ),
            ("remove-upstream", cleared),
            ("delete-branch", checked_out("review")),
        ]
    };
    open_branch(&app, cx, "review").await;
    assert_eq!(refused_actions(cx), held(unset.clone()));
    fixture_git(&repository, &["branch", "--set-upstream-to=main", "review"]);
    open_branch(&app, cx, "review").await;
    assert_eq!(
        refused_actions(cx),
        held(format!(
            "{elsewhere} Remove its upstream from that worktree."
        ))
    );

    open_branch(&app, cx, "main").await;
    assert_eq!(
        refused_actions(cx),
        [
            ("remove-upstream", unset.clone()),
            ("delete-branch", checked_out("main")),
        ]
    );

    open_branch(&app, cx, "ahead").await;
    assert_eq!(
        refused_actions(cx),
        [
            ("remove-upstream", unset),
            (
                "delete-branch",
                "1 commit is outside main. Merge or preserve that work before deleting this branch."
                    .to_owned()
            ),
        ]
    );

    remote(cx);
    assert_eq!(refused_actions(cx), Vec::<(&str, String)>::new());
    fixture_git(&repository, &["checkout", "--quiet", "--detach"]);
    reopen(&app, cx, &repository).await;
    remote(cx);
    let detached = "HEAD is detached. Switch to a branch first.".to_owned();
    assert_eq!(
        refused_actions(cx),
        [
            ("merge-remote-branch", detached.clone()),
            ("rebase-remote-branch", detached),
        ]
    );
    close_dialog(cx);
}

/// A tracking branch created through its form with Switch to the new branch
/// cleared confirms in place (DESIGN.md's Git feedback in place): no control
/// reads a word, as the picker still names the branch it named, so the
/// outcome, "Created kept · tracks origin/kept", is announced once and
/// recorded in the status line while the window-wide notice stays down. The
/// form's choice reaches the review, which says the current branch stays
/// selected and offers Create branch, and Git creates the branch without
/// switching to it.
#[gpui::test]
async fn a_tracking_branch_created_without_the_switch_confirms_in_place(cx: &mut TestAppContext) {
    use crate::copy_feedback::tests::announced;
    use crate::views::tests::{fixture_git, history_script, settle};
    let (fixture, app, cx) =
        crate::views::tests::history_window_from(cx, history_script(3, 1)).await;
    // The form's dialog slides in on the real clock: at rest from its first
    // frame, its checkbox lies where the click lands.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
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
    fixture_git(
        &repository,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    // Pushing to the named remote also stores its remote-tracking ref.
    fixture_git(
        &repository,
        &["push", "--quiet", "origin", "main:refs/heads/kept"],
    );
    reopen(&app, cx, &repository).await;
    let confirm = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx), "a dialog is open");
            window.dispatch_action(
                Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
                cx,
            )
        })
    };

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let reference = "refs/remotes/origin/kept".to_owned();
            app.open_branch_form(BranchFormMode::Track(reference), window, cx)
        })
    });
    draw(cx);
    let switch = rendered(cx, "checkout-new-tracking-branch");
    cx.simulate_click(switch.center(), gpui::Modifiers::default());
    draw(cx);
    confirm(cx);
    let read = app.update(cx, |app, _| app.branch_actions.task.take());
    read.expect("the review is prepared").await;
    draw(cx);
    let consequences = native_accessibility::written::get("operation-consequences")
        .and_then(|written| written.node.value().map(str::to_owned))
        .unwrap_or_default();
    assert!(
        consequences.starts_with("Create local branch kept at ")
            && consequences.contains("The current branch stays selected."),
        "the review keeps the form's choice: {consequences:?}"
    );
    // Every announcement and alert takes the next generation, which names
    // its node (`copy_feedback::announcement`): from one said just before
    // the review is confirmed, the outcome's node is the next one only if
    // nothing else, the outcome again included, was said in between.
    let latest = |cx: &mut VisualTestContext| match announced(cx) {
        Some((ElementId::NamedInteger(_, generation), text)) => (generation, text),
        other => panic!("an announcement is drawn: {other:?}"),
    };
    cx.update(|window, cx| crate::copy_feedback::announce(window, cx, "Before the review"));
    draw(cx);
    let (before, _) = latest(cx);
    confirm(cx);
    draw(cx);
    let write = app
        .update(cx, |app, _| app.operation_task.take())
        .expect("the write");
    write.await;
    settle(&app, cx).await;

    let outcome = "Created kept · tracks origin/kept";
    let (after, said) = latest(cx);
    assert_eq!(
        (after - before, said.as_str()),
        (1, outcome),
        "the outcome is announced once, and nothing else is"
    );
    app.read_with(cx, |app, _| {
        assert_eq!(app.operation_error, None);
        assert_eq!(
            app.operation_notice, None,
            "the window-wide notice stays down"
        );
        assert_eq!(app.status_line(), outcome, "the status line records it");
        let branch = app.work_status.as_ref().and_then(|s| s.branch.as_deref());
        assert_eq!(branch, Some("main"), "Git switched nothing");
    });
    assert!(cx.debug_bounds("operation-notice-summary").is_none());
    let repo = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
    let kept = repo.branch_plan("kept").expect("the branch was created");
    assert_eq!(kept.upstream.as_deref(), Some("refs/remotes/origin/kept"));
}

/// The branch picker's label, accessible name and tooltip are built only
/// when what they follow from changes ([`PickerInputs`]): the branch, HEAD's
/// commit where none is checked out, and a switch's confirmation. Each frame
/// cloned the branch's name and formatted its label, name and tooltip,
/// however steady. Frames at rest build none of it; another branch and a
/// detached HEAD build it once each; and the compact bar's forms are kept
/// with the full bar's, so the compact row at 18 pt in 700 × 490, whose
/// picker names the commit, and in 461 × 490, where it says "Detached" in
/// the line's place and carries the line's advice, build none of it. Each
/// is drawn as it now reads.
#[gpui::test]
async fn the_branch_picker_builds_its_text_only_when_what_it_shows_changes(
    cx: &mut TestAppContext,
) {
    use crate::views::tests::{history_window, hold_quiet_reads_for_test};
    const SWITCH: &str = "Switch branch and repository actions";
    // The thread's kept text from an earlier run, as another seed's, would
    // hold this run's branch and commit already.
    PICKER_TEXTS.with_borrow_mut(|texts| *texts = crate::views::Measured::new());
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    cx.simulate_resize(size(px(1480.), px(800.)));
    draw(cx);
    let built = || PICKER_TEXTS_BUILT.get();
    let at_rest = |at: &str, cx: &mut VisualTestContext| {
        let before = built();
        for _ in 0..3 {
            app.update(cx, |_, cx| cx.notify());
            draw(cx);
        }
        assert_eq!(built() - before, 0, "{at}: frames at rest build none of it");
    };
    // What the picker draws, its label and tooltip, and the accessible name
    // its node is written with, the same in every form.
    let drawn = |cx: &mut VisualTestContext| {
        let label = PICKER_LABEL.with(|label| label.borrow().clone());
        let tooltip = crate::views::CONTROL_TOOLTIPS
            .with(|tooltips| tooltips.borrow().get("current-branch").cloned());
        let picker = app.clone();
        let node = crate::views::tests::view_node(cx, move |window, cx| {
            use gpui::Element as _;
            let button = picker.update(cx, |app, cx| app.branch_picker(false, false, None, cx));
            let element =
                gpui::RenderOnce::render(button.base(window, cx), window, cx).into_element();
            let mut node =
                gpui::accesskit::Node::new(element.a11y_role().expect("the picker's role"));
            element.write_a11y_info(&mut node);
            node
        });
        (label, tooltip, node.label().map(str::to_owned))
    };
    fn status(app: &mut GitTurtle) -> &mut gitturtle_core::RepositoryStatus {
        Arc::make_mut(app.work_status.as_mut().expect("the status is read"))
    }
    let size_to = |cx: &mut VisualTestContext, text: u8, width: f32, height: f32| {
        cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
    };

    at_rest("History at 1480 × 800", cx);
    let before = built();
    app.update(cx, |app, cx| {
        status(app).branch = Some("topic".into());
        cx.notify();
    });
    draw(cx);
    assert_eq!(built() - before, 1, "another branch builds it once");
    assert_eq!(
        drawn(cx),
        (
            Some("topic".into()),
            Some(format!("topic · {SWITCH}")),
            Some("Current branch: topic. Switch or manage branches".into()),
        ),
        "the picker reads the new branch"
    );
    at_rest("another branch", cx);

    let head = app.read_with(cx, |app, _| {
        (app.work_status.as_ref())
            .and_then(|status| status.head.clone())
            .expect("HEAD has a commit")
    });
    let short = &head[..7];
    let named = format!("Current branch: Detached HEAD at {short}. Switch or manage branches");
    let before = built();
    app.update(cx, |app, cx| {
        status(app).branch = None;
        cx.notify();
    });
    draw(cx);
    assert_eq!(built() - before, 1, "a detached HEAD builds it once");
    assert_eq!(
        drawn(cx),
        (
            Some("Detached HEAD".into()),
            Some(format!("Detached HEAD at {short} · {SWITCH}")),
            Some(named.clone()),
        ),
        "the full bar's picker names the commit in its tooltip and name"
    );
    at_rest("a detached HEAD", cx);

    let before = built();
    size_to(cx, 18, 700., 490.);
    let compact = drawn(cx);
    size_to(cx, 18, 461., 490.);
    let narrow = drawn(cx);
    assert_eq!(built() - before, 0, "the compact forms build none of it");
    assert_eq!(
        compact,
        (
            Some(short.to_owned()),
            Some(format!("Detached HEAD at {short} · {SWITCH}")),
            Some(named.clone()),
        ),
        "the compact row's picker is labelled by the commit"
    );
    assert_eq!(
        narrow,
        (
            Some(crate::empty_state::DETACHED.to_owned()),
            Some(format!(
                "Detached HEAD at {short} · Create a branch here to keep new commits on it · {SWITCH}"
            )),
            Some(named),
        ),
        "where the line leaves, the picker says its word and carries its advice"
    );
    at_rest("the compact row in 461 × 490", cx);
    size_to(cx, 13, 1480., 800.);
}
