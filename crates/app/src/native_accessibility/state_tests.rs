//! Every empty and edge state's line is static text an assistive technology
//! reads by the whole line, where the window draws it among the controls
//! around it (`empty_state::EmptyState`, `native_accessibility::static_text`).
//! Each line was a label named only by its label property, which AccessKit's
//! adapters do not read for a label, so AT-SPI reached every one of them
//! without a name: "No remote", a detached HEAD, "No upstream", no commits
//! yet, a clean working tree, empty Tags and Stashes lists and the GitHub
//! panel without a GitHub remote.
//!
//! The test platform builds no accessibility tree, so each check assembles
//! the part GPUI would from the nodes the lines and the wrappers around them
//! wrote as they last prepainted (`written`): the page beneath any dialog,
//! the lists that clip what they hold, and the lines, each under the nearest
//! of them it was drawn in, in the order GPUI wrote them.

use crate::native_accessibility::{reachable, written};
use crate::views::tests::{draw, fixture_git, history_script, settle, styled_history_window};
use crate::*;
use core::prelude::v1::test;

/// A window on the repository `input` imports, and the repository's root.
async fn window<'a>(
    cx: &'a mut TestAppContext,
    input: &str,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    &'a mut VisualTestContext,
    PathBuf,
) {
    let (fixture, app, cx) = styled_history_window(cx, input.as_bytes(), |root| root).await;
    let root = app.read_with(cx, |app, _| app.path.clone().unwrap());
    (fixture, app, cx, root)
}

/// Reads the working tree, its branch and remotes again, as Refresh does.
async fn refresh(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    settle(app, cx).await;
}

/// Draws, letting the reader threads answer, until `done` holds.
fn wait(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..400 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        draw(cx);
        if done(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("{what} never happened");
}

/// The sizes each frame is read at: the default size in a wide window, and
/// the largest in a short half tile, where the Git action bar is compact.
const SIZES: [(u8, f32, f32); 2] = [(13, 1280., 800.), (18, 461., 490.)];

/// At each of [`SIZES`], the state lines `lines`, each its state's id and
/// what an assistive technology reads it as, in the order they are written
/// in the tree, which is the order the window reads them in: each a label
/// named by its whole line, reached through AccessKit's filter and, on
/// Linux, the AT-SPI adapter, in that order. None of the lines `covered`
/// names, which lie beneath an open dialog, is reached.
fn read(cx: &mut VisualTestContext, what: &str, lines: &[(&str, &str)], covered: &[&str]) {
    read_at(cx, what, lines, covered, &SIZES);
}

/// [`read`] at `sizes`, each an interface size and a window's width and
/// height.
fn read_at(
    cx: &mut VisualTestContext,
    what: &str,
    lines: &[(&str, &str)],
    covered: &[&str],
    sizes: &[(u8, f32, f32)],
) {
    for &(text, width, height) in sizes {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        written::forget();
        draw(cx);
        let at = format!("{what} at {text} pt in {width} × {height}");
        let key = |id: &str| format!("{id}-line");
        let mut tree = written::Assembly::default();
        let mut orders = Vec::new();
        for (id, name) in lines {
            let line = written::get(&key(id)).unwrap_or_else(|| panic!("{at}: {id} wrote no node"));
            assert_eq!(line.node.role(), Role::Label, "{at}: {id}");
            assert_eq!(line.node.value(), Some(*name), "{at}: {id}'s text");
            orders.push(line.order);
            tree = tree.written(&key(id), None);
        }
        assert!(
            orders.is_sorted(),
            "{at}: the tree holds {lines:?} out of reading order: {orders:?}"
        );
        for id in covered {
            tree = tree.written(&key(id), None);
        }
        let viewport = cx.update(|window, _| window.viewport_size());
        let scale = f64::from(cx.update(|window, _| window.scale_factor()));
        let window = gpui::accesskit::Rect {
            x0: 0.,
            y0: 0.,
            x1: f64::from(f32::from(viewport.width)) * scale,
            y1: f64::from(f32::from(viewport.height)) * scale,
        };
        let update = tree.tree(window, None);
        let names = |found: Vec<(bool, Option<String>)>| -> Vec<String> {
            found
                .into_iter()
                .filter(|(label, _)| *label)
                .filter_map(|(_, name)| name)
                .collect()
        };
        let expected: Vec<String> = lines.iter().map(|(_, name)| (*name).to_owned()).collect();
        let filtered = names(
            reachable(update.clone())
                .into_iter()
                .map(|node| (node.role == Role::Label, node.name))
                .collect(),
        );
        assert_eq!(filtered, expected, "{at}: through AccessKit's filter");
        #[cfg(target_os = "linux")]
        {
            use crate::native_accessibility::atspi_state_tests::reached;
            use accesskit_atspi_common::Role as AtspiRole;
            let served = names(
                reached(update)
                    .into_iter()
                    .map(|node| (node.role == AtspiRole::Label, Some(node.name)))
                    .collect(),
            );
            assert_eq!(served, expected, "{at}: through the AT-SPI adapter");
        }
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    cx.simulate_resize(size(px(1280.), px(800.)));
    draw(cx);
}

/// The Git action bar's edges, from a repository with no remote to one
/// whose branch has no upstream and a detached HEAD, and an empty
/// repository's History and Changes.
#[gpui::test]
async fn every_repository_edge_is_read_by_its_line(cx: &mut TestAppContext) {
    let (_fixture, app, cx, root) = window(cx, &history_script(1, 1)).await;
    read(cx, "No remote", &[("branch-edge", "No remote")], &[]);
    let explanation = written::get("branch-edge-line")
        .and_then(|line| line.node.description().map(str::to_owned));
    assert_eq!(
        explanation.as_deref(),
        Some("This repository has no remote. Add one to fetch, pull and push."),
        "the line's tooltip is its description"
    );

    fixture_git(&root, &["remote", "add", "origin", "../origin.git"]);
    refresh(&app, cx).await;
    // The caption goes where the bar has no room for it; the name keeps it.
    read(
        cx,
        "No upstream",
        &[("branch-edge", "No upstream · Push sets one")],
        &[],
    );

    fixture_git(&root, &["checkout", "--detach"]);
    refresh(&app, cx).await;
    let head = app.read_with(cx, |app, _| {
        app.work_status
            .as_ref()
            .and_then(|status| status.head.clone())
            .unwrap()
    });
    let detached = format!("Detached at {}", &head[..7]);
    read(cx, "Detached", &[("branch-edge", &detached)], &[]);
}

/// An empty repository's History and Changes, without and with a remote:
/// the bar's line comes first, then the page's, the preview's before the
/// list's in the inspector beside it, as the window lays them out.
#[gpui::test]
async fn an_empty_repository_reads_its_lines_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx, root) = window(cx, "done\n").await;
    const NO_COMMITS: &str = "No commits yet · Your first commit starts in Changes";
    read(
        cx,
        "No commits",
        &[
            ("branch-edge", "No remote"),
            ("history-no-commits", NO_COMMITS),
        ],
        &[],
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    let preview = "No files yet · Files you add appear here for the first commit";
    read(
        cx,
        "Changes with no commits",
        &[
            ("branch-edge", "No remote"),
            ("working-preview-clean", preview),
            ("working-clean", "No files yet"),
        ],
        &[],
    );
    fixture_git(&root, &["remote", "add", "origin", "../origin.git"]);
    refresh(&app, cx).await;
    read(
        cx,
        "Changes with a remote and no commits",
        &[
            ("branch-edge", "No commits to push yet"),
            ("working-preview-clean", preview),
            ("working-clean", "No files yet"),
        ],
        &[],
    );
}

/// A clean working tree, empty Tags and Stashes lists and the GitHub panel
/// without a GitHub remote; while a dialog is open, the page's own line
/// beneath it is out of reach.
#[gpui::test]
async fn empty_lists_and_panels_are_read_by_their_lines(cx: &mut TestAppContext) {
    let (_fixture, app, cx, root) = window(cx, &history_script(1, 1)).await;
    fixture_git(&root, &["reset", "--hard"]);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    read(
        cx,
        "A clean working tree",
        &[
            ("branch-edge", "No remote"),
            (
                "working-preview-clean",
                "Working tree clean · Edits you make appear here",
            ),
            ("working-clean", "Working tree clean"),
        ],
        &[],
    );

    cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
    wait(cx, "the Tags list", |cx| {
        cx.debug_bounds("tag-list-empty-line").is_some()
    });
    read(
        cx,
        "Tags",
        &[(
            "tag-list-empty",
            "No local tags yet · Create tag… names a commit",
        )],
        &["branch-edge"],
    );
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx);

    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    wait(cx, "the stash list", |cx| {
        cx.debug_bounds("stash-list-empty-line").is_some()
    });
    read(
        cx,
        "Stashes",
        &[(
            "stash-list-empty",
            "No saved stashes · Save one from the Actions menu in Changes",
        )],
        &["branch-edge"],
    );
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx);

    cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
    wait(cx, "the panel's local read", |cx| {
        cx.debug_bounds("github-list-empty-line").is_some()
    });
    // Follow-up: in the short half tile at 18 pt the panel's list lies
    // below its fold, where AccessKit's filter rightly leaves the line out;
    // the tall one draws it.
    read_at(
        cx,
        "The GitHub panel",
        &[(
            "github-list-empty",
            "No GitHub remote · Name the repository to load its pull requests",
        )],
        &["branch-edge"],
        &[SIZES[0], (18, 461., 980.)],
    );
}
