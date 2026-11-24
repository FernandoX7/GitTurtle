use super::*;
use crate::tags::tests::{draw, git, rendered, set_interface_text_size};
use core::prelude::v1::test;
use std::fs;
use std::path::Path;

fn commit_file(repo: &Path, name: &str, body: &str, message: &str) {
    let target = repo.join(name);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(target, body).unwrap();
    git(repo, &["add", name]);
    git(repo, &["commit", "--quiet", "-m", message]);
}

fn four_commit_repo(dir: &Path) -> GitRepository {
    let repo = crate::views::tests::fixture_repository(dir.join("repo"));
    commit_file(repo.path(), "a.txt", "a1\n", "Initial commit");
    commit_file(repo.path(), "b.txt", "b1\n", "Second commit");
    commit_file(repo.path(), "c.txt", "c1\n", "Third commit");
    commit_file(repo.path(), "d.txt", "d1\n", "Fourth commit");
    repo
}

async fn settle_app(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    crate::views::tests::settle(app, cx).await;
    draw(cx);
}

async fn open_in_window<'a>(
    cx: &'a mut TestAppContext,
    repo: &GitRepository,
) -> (Entity<GitTurtle>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    crate::views::tests::init_app(cx);
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        shortcuts::bind_keys(cx);
        native_accessibility::bind_keys(cx);
    });
    let path = repo.path().to_owned();
    let captured: std::rc::Rc<std::cell::RefCell<Option<Entity<GitTurtle>>>> = Default::default();
    let observed = captured.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| {
            let app = GitTurtle::new(
                Some(path),
                Preferences::default(),
                repository_tabs::Session::default(),
                activity::State::default(),
                recovery_drafts::State::default(),
                window,
                cx,
            );
            futures::executor::block_on(app.preferences_writer.submit(|| Ok(())))
                .expect("startup preferences worker replied")
                .expect("startup preferences queue drained");
            app
        });
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = observed.borrow_mut().take().unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
    cx.simulate_resize(size(px(1200.), px(760.)));
    settle_app(&app, cx).await;
    (app, cx)
}

#[gpui::test]
async fn shift_click_selects_commit_range_loads_passive_diff_and_stays_in_history(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let repo = four_commit_repo(dir.path());
    let (app, cx) = open_in_window(cx, &repo).await;

    let (oid0, oid2) = app.read_with(cx, |app, _| {
        assert_eq!(app.mode, WorkspaceMode::History);
        assert_eq!(app.selected_commit, Some(0));
        assert!(app.history_range.is_none());
        (app.commits[0].oid.clone(), app.commits[2].oid.clone())
    });

    // Shift-clicking the same already-selected single commit stays single.
    let row0 = rendered(cx, "history-row-0");
    cx.simulate_click(row0.center(), Modifiers::shift());
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none(), "same commit stays single");
        assert_eq!(app.selected_commit, Some(0));
    });

    // Shift-click commit 2 to compare older (2) .. newer (0).
    let row2 = rendered(cx, "history-row-2");
    cx.simulate_click(row2.center(), Modifiers::shift());
    settle_app(&app, cx).await;

    app.read_with(cx, |app, _| {
        assert_eq!(
            app.mode,
            WorkspaceMode::History,
            "selecting a History range must load files passively without opening Compare"
        );
        let range = app.history_range.as_ref().expect("active history range");
        assert_eq!(range.anchor_oid, oid0);
        assert_eq!(range.target_oid, oid2);
        assert_eq!(range.before_oid, oid2, "older commit is before by default");
        assert_eq!(range.after_oid, oid0, "newer commit is after by default");
        assert_eq!(range.commit_count, 3);
        assert!(!range.swapped);
        let paths: Vec<String> = app
            .files
            .iter()
            .map(|f| f.path().to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths, vec!["c.txt", "d.txt"]);
    });

    assert!(cx.debug_bounds("history-range-inspector").is_some());
    assert!(cx.debug_bounds("history-range-header").is_some());
    assert!(cx.debug_bounds("history-range-swap").is_some());
    assert!(cx.debug_bounds("history-range-clear").is_some());

    // Shift-clicking back on the anchor commit collapses cleanly to single commit.
    let row0 = rendered(cx, "history-row-0");
    cx.simulate_click(row0.center(), Modifiers::shift());
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(
            app.history_range.is_none(),
            "shift-clicking anchor collapses back to single commit"
        );
        assert_eq!(app.selected_commit, Some(0));
    });

    // Select range again, then ordinary click returns to single-commit inspection.
    let row2 = rendered(cx, "history-row-2");
    cx.simulate_click(row2.center(), Modifiers::shift());
    settle_app(&app, cx).await;
    assert!(app.read_with(cx, |app, _| app.history_range.is_some()));

    let row1 = rendered(cx, "history-row-1");
    cx.simulate_click(row1.center(), Modifiers::default());
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(
            app.history_range.is_none(),
            "ordinary click exits range compare"
        );
        assert_eq!(app.selected_commit, Some(1));
        let paths: Vec<String> = app
            .files
            .iter()
            .map(|f| f.path().to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths, vec!["c.txt"]);
    });
}

#[gpui::test]
async fn shift_arrows_extend_shrink_and_collapse_history_range(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let repo = four_commit_repo(dir.path());
    let (app, cx) = open_in_window(cx, &repo).await;

    cx.update(|window, cx| {
        let focus = app.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    draw(cx);

    cx.simulate_keystrokes("shift-down");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        let range = app.history_range.as_ref().expect("range after shift-down");
        assert_eq!(range.commit_count, 2);
        assert_eq!(range.before_oid, app.commits[1].oid);
        assert_eq!(range.after_oid, app.commits[0].oid);
        assert_eq!(app.mode, WorkspaceMode::History);
    });

    cx.simulate_keystrokes("shift-down");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        let range = app
            .history_range
            .as_ref()
            .expect("range after second shift-down");
        assert_eq!(range.commit_count, 3);
        assert_eq!(range.before_oid, app.commits[2].oid);
        assert_eq!(range.after_oid, app.commits[0].oid);
    });

    cx.simulate_keystrokes("shift-up");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        let range = app.history_range.as_ref().expect("range after shift-up");
        assert_eq!(range.commit_count, 2);
        assert_eq!(range.before_oid, app.commits[1].oid);
    });

    // Stepping back onto the anchor collapses cleanly to single commit.
    cx.simulate_keystrokes("shift-up");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none());
        assert_eq!(app.selected_commit, Some(0));
    });

    // Re-extend and confirm plain Down arrow returns cleanly to single commit.
    cx.simulate_keystrokes("shift-down");
    settle_app(&app, cx).await;
    assert!(app.read_with(cx, |app, _| app.history_range.is_some()));

    cx.simulate_keystrokes("down");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none());
        assert_eq!(app.selected_commit, Some(2));
    });
}

#[gpui::test]
async fn swap_clear_escape_and_compare_back_preserve_history_range_context(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let repo = four_commit_repo(dir.path());
    let (app, cx) = open_in_window(cx, &repo).await;

    let row2 = rendered(cx, "history-row-2");
    cx.simulate_click(row2.center(), Modifiers::shift());
    settle_app(&app, cx).await;

    let (oid0, oid2) = app.read_with(cx, |app, _| {
        assert_eq!(app.files[0].status.letter(), "A");
        (app.commits[0].oid.clone(), app.commits[2].oid.clone())
    });

    // Swap inverts before/after and flips added files to deleted.
    let swap = rendered(cx, "history-range-swap");
    cx.simulate_click(swap.center(), Modifiers::default());
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        let range = app.history_range.as_ref().expect("swapped range");
        assert!(range.swapped);
        assert_eq!(range.before_oid, oid0);
        assert_eq!(range.after_oid, oid2);
        assert_eq!(app.mode, WorkspaceMode::History);
        assert_eq!(app.files[0].status.letter(), "D");
    });

    // Explicit file activation enters Compare with the exact range origins.
    let file_row = rendered(cx, "selected-changed-file");
    cx.simulate_click(file_row.center(), Modifiers::default());
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert_eq!(app.mode, WorkspaceMode::Compare);
        assert!(app.history_range.is_some());
        assert_eq!(
            app.preview_origins(),
            markdown_view::Origins::revisions(Some(oid0.clone()), Some(oid2.clone()))
        );
    });

    // Back / Escape from Compare restores History and retains the active range.
    cx.simulate_keystrokes("escape");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert_eq!(app.mode, WorkspaceMode::History);
        assert!(
            app.history_range.is_some(),
            "Back from Compare retains History range context"
        );
        assert!(!app.files.is_empty());
    });

    // Clear button in Compare mode clears the range and moves keyboard focus
    // onto the file list instead of stranding focus on the unmounted button.
    let file_row = rendered(cx, "selected-changed-file");
    cx.simulate_click(file_row.center(), Modifiers::default());
    settle_app(&app, cx).await;
    assert_eq!(app.read_with(cx, |app, _| app.mode), WorkspaceMode::Compare);
    assert!(app.read_with(cx, |app, _| app.history_range.is_some()));
    let clear = rendered(cx, "history-range-clear");
    cx.simulate_click(clear.center(), Modifiers::default());
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none());
        assert_eq!(app.mode, WorkspaceMode::Compare);
        assert!(app.pane == Pane::Files);
    });
    assert!(
        cx.update(|window, cx| app.read(cx).file_focus.is_focused(window)),
        "clearing range in Compare keeps keyboard focus on the changed-file list"
    );

    // Return to History, re-select range, and verify Escape in History clears
    // the active range back to single-commit inspection.
    cx.simulate_keystrokes("escape");
    settle_app(&app, cx).await;
    let row0 = rendered(cx, "history-row-0");
    cx.simulate_click(row0.center(), Modifiers::default());
    settle_app(&app, cx).await;
    let row2 = rendered(cx, "history-row-2");
    cx.simulate_click(row2.center(), Modifiers::shift());
    settle_app(&app, cx).await;
    assert!(app.read_with(cx, |app, _| app.history_range.is_some()));

    cx.simulate_keystrokes("escape");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none());
        assert_eq!(app.mode, WorkspaceMode::History);
    });

    // Re-select range and verify secondary-click (Cmd/Ctrl) reloads
    // single-commit files instead of leaving stale range files behind.
    let row0 = rendered(cx, "history-row-0");
    cx.simulate_click(row0.center(), Modifiers::default());
    settle_app(&app, cx).await;
    let row2 = rendered(cx, "history-row-2");
    cx.simulate_click(row2.center(), Modifiers::shift());
    settle_app(&app, cx).await;
    assert_eq!(app.read_with(cx, |app, _| app.files.len()), 2);

    let secondary = cx.update(|_, cx| Modifiers::secondary_key(cx));
    let row1 = rendered(cx, "history-row-1");
    cx.simulate_click(row1.center(), secondary);
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none());
        assert_eq!(
            app.selected_compare_commit.as_deref(),
            Some(app.commits[1].oid.as_str())
        );
        assert_eq!(
            app.files.len(),
            1,
            "secondary-click reloads selected commit's single-commit files"
        );
    });

    // Clear button in History also clears the range cleanly.
    let row0 = rendered(cx, "history-row-0");
    cx.simulate_click(row0.center(), Modifiers::default());
    settle_app(&app, cx).await;
    let row2 = rendered(cx, "history-row-2");
    cx.simulate_click(row2.center(), Modifiers::shift());
    settle_app(&app, cx).await;
    assert!(app.read_with(cx, |app, _| app.history_range.is_some()));

    let clear = rendered(cx, "history-range-clear");
    cx.simulate_click(clear.center(), Modifiers::default());
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none());
        assert_eq!(app.mode, WorkspaceMode::History);
        assert!(app.pane == Pane::History);
    });
}

#[gpui::test]
async fn stale_range_reply_is_discarded_by_generation_and_oid_check(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let repo = four_commit_repo(dir.path());
    let (app, cx) = open_in_window(cx, &repo).await;

    let (oid0, oid1, oid2, repo_handle) = app.read_with(cx, |app, _| {
        (
            app.commits[0].oid.clone(),
            app.commits[1].oid.clone(),
            app.commits[2].oid.clone(),
            app.repository.clone().unwrap(),
        )
    });

    // Build a real RevisionComparison for oid2..oid0 while active state is on
    // single commit 1 (or a different range oid1..oid0).
    let stale = repo_handle
        .compare_revisions(
            &oid2,
            &oid0,
            gitturtle_core::ComparisonMode::Endpoints,
            &gitturtle_core::HistoryCancellation::default(),
        )
        .unwrap();
    assert_eq!(stale.files.len(), 2);

    // Select single commit 1 and settle.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.select_commit(1, window, cx));
    });
    settle_app(&app, cx).await;

    // Delivering stale Output::RevisionComparison must not touch single-commit files.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.receive(Output::RevisionComparison(stale.clone()), window, cx);
        });
    });
    draw(cx);
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none());
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.files[0].path(), Path::new("c.txt"));
    });

    // Even when a DIFFERENT range (oid1..oid0) is active, oid2..oid0 reply is ignored.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.select_commit(0, window, cx));
    });
    settle_app(&app, cx).await;
    cx.update(|window, cx| {
        app.update(cx, |app, cx| app.select_history_range(1, window, cx));
    });
    settle_app(&app, cx).await;

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.receive(Output::RevisionComparison(stale), window, cx);
        });
    });
    draw(cx);
    app.read_with(cx, |app, _| {
        let range = app.history_range.as_ref().expect("oid1..oid0 range");
        assert_eq!(range.before_oid, oid1);
        assert_eq!(range.after_oid, oid0);
        assert_eq!(app.files.len(), 1);
        assert_eq!(app.files[0].path(), Path::new("d.txt"));
    });
}

#[gpui::test]
async fn filtered_search_merge_detached_narrow_viewport_and_accesskit(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let repo = four_commit_repo(dir.path());
    // Create a feature branch, merge it, then detach HEAD.
    git(
        repo.path(),
        &["checkout", "--quiet", "-b", "feature", "HEAD~2"],
    );
    commit_file(repo.path(), "feature.txt", "feat\n", "Feature work");
    git(repo.path(), &["checkout", "--quiet", "main"]);
    git(
        repo.path(),
        &[
            "merge",
            "--quiet",
            "--no-ff",
            "feature",
            "-m",
            "Merge feature",
        ],
    );
    git(repo.path(), &["checkout", "--quiet", "--detach", "HEAD"]);

    let (app, cx) = open_in_window(cx, &repo).await;

    // Shift-click across detached HEAD merge commit to an older commit.
    let row2 = rendered(cx, "history-row-2");
    cx.simulate_click(row2.center(), Modifiers::shift());
    settle_app(&app, cx).await;

    // Verify AccessKit nodes on range header and commit rows.
    native_accessibility::written::forget();
    draw(cx);
    let (oid_before, oid_after, oid_mid) = app.read_with(cx, |app, _| {
        let range = app.history_range.as_ref().expect("detached merge range");
        assert_eq!(range.commit_count, 3);
        (
            range.before_oid.clone(),
            range.after_oid.clone(),
            app.commits[app.visible[1]].oid.clone(),
        )
    });
    let header_node = native_accessibility::written::get("history-range-header")
        .expect("history-range-header recorded in AccessKit");
    assert_eq!(header_node.node.role(), Role::Group);
    let header_label = header_node.node.label().unwrap_or_default();
    assert!(
        header_label.contains("Comparing ") && header_label.contains("3 commits"),
        "unexpected range header label: {header_label}"
    );
    let before_node =
        native_accessibility::written::get(&views::commit_row_id(&oid_before).to_string())
            .expect("before commit row recorded");
    assert!(before_node.node.is_selected() == Some(true));
    assert!(
        before_node
            .node
            .label()
            .unwrap_or_default()
            .contains("range start"),
        "before row label: {:?}",
        before_node.node.label()
    );
    let after_node =
        native_accessibility::written::get(&views::commit_row_id(&oid_after).to_string())
            .expect("after commit row recorded");
    assert!(after_node.node.is_selected() == Some(true));
    assert!(
        after_node
            .node
            .label()
            .unwrap_or_default()
            .contains("range end"),
        "after row label: {:?}",
        after_node.node.label()
    );
    let mid_node = native_accessibility::written::get(&views::commit_row_id(&oid_mid).to_string())
        .expect("middle commit row recorded");
    assert!(mid_node.node.is_selected() == Some(true));
    assert!(
        mid_node
            .node
            .label()
            .unwrap_or_default()
            .contains("in comparison range"),
        "middle row label: {:?}",
        mid_node.node.label()
    );

    // Narrow viewport bounds check (620 × 600 and 461 × 490 half-tile) at 13 pt and 18 pt:
    // Swap & Clear stay inside inspector and Changed files keeps vertical room.
    for (viewport, font_size) in [
        (size(px(620.), px(600.)), 13_u8),
        (size(px(620.), px(600.)), 18),
        (size(px(461.), px(490.)), 13),
        (size(px(461.), px(490.)), 18),
    ] {
        cx.simulate_resize(viewport);
        set_interface_text_size(cx, &app, font_size);
        draw(cx);
        let inspector = rendered(cx, "history-range-inspector");
        let header = rendered(cx, "history-range-header");
        let swap = rendered(cx, "history-range-swap");
        let clear = rendered(cx, "history-range-clear");
        let files = rendered(cx, "commit-inspector-files");
        for (name, bounds) in [("header", header), ("swap", swap), ("clear", clear)] {
            assert!(
                bounds.left() >= inspector.left() - px(1.)
                    && bounds.right() <= inspector.right() + px(1.),
                "{viewport:?} {font_size}pt {name} horizontally overflowed inspector: {bounds:?} vs {inspector:?}"
            );
        }
        assert!(
            files.size.height >= px(80.),
            "{viewport:?} {font_size}pt changed-files pane starved: {files:?}"
        );
    }

    // Filtered/searched History retains & restores normal range and lets Esc
    // clear an in-search range without clearing the search query first.
    cx.simulate_resize(size(px(1200.), px(760.)));
    set_interface_text_size(cx, &app, 13);
    draw(cx);
    cx.update(|window, cx| {
        let search = app.read(cx).search.clone();
        search.update(cx, |input, cx| input.set_value("commit", window, cx));
        app.update(cx, |app, cx| app.history_query_changed(window, cx));
    });
    history_search::settle_history_search(&app, cx).await;
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_search_active());
        assert!(app.visible.len() >= 2, "search matched multiple commits");
        assert!(app.history_range.is_none());
    });

    // Range compare inside search results.
    let search_row1 = rendered(cx, "history-row-1");
    cx.simulate_click(search_row1.center(), Modifiers::shift());
    settle_app(&app, cx).await;
    assert!(app.read_with(cx, |app, _| app.history_range.is_some()));

    // First Escape clears the search range while keeping search active.
    cx.simulate_keystrokes("escape");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.history_range.is_none());
        assert!(
            app.history_search_active(),
            "first Escape clears range without wiping search"
        );
    });

    // Second Escape clears search and restores the normal History range.
    cx.simulate_keystrokes("escape");
    settle_app(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(!app.history_search_active());
        let restored = app
            .history_range
            .as_ref()
            .expect("normal history range restored after leaving search");
        assert_eq!(restored.before_oid, oid_before);
        assert_eq!(restored.after_oid, oid_after);
    });
}
