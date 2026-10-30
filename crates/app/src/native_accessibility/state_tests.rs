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
use crate::recovery::StashBrowser;
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

/// Opens Tags and draws it once its list has landed: the list is read on
/// the app's operation executor, a thread of its own, and the dialog opens
/// with the reply, so the test waits for that read
/// (`tags::tests::read_lands`) rather than for wall-clock time.
async fn open_tags(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
    crate::tags::tests::read_lands(app, cx).await;
    draw(cx);
}

/// Opens Stashes and returns its browser once its saved stashes have landed
/// (`StashBrowser::lands`): they are read on the browser's own thread.
fn open_stashes(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Entity<StashBrowser> {
    let browser = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
        .expect("Stashes opens");
    StashBrowser::lands(&browser, cx);
    browser
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

    open_tags(&app, cx).await;
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

    open_stashes(&app, cx);
    assert!(
        cx.debug_bounds("stash-list-empty-line").is_some(),
        "the empty stash list is drawn"
    );
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
    // Name repository is drawn once the local read finds no GitHub remote;
    // the line is there before it lands, saying no pull requests are shown.
    GitTurtle::github_local_read_lands(&app, cx);
    assert!(
        cx.debug_bounds("github-choose-destination").is_some(),
        "the panel's local read found no GitHub remote"
    );
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

/// Every label the frame writes on `screen`, each named, and among them the
/// labels `expected`, each by the element ID it was written with: a label
/// named only by its label property is written as no static text, so it is
/// missing here, as an assistive technology reached it without a name.
fn labels_named(cx: &mut VisualTestContext, screen: &str, expected: &[&str]) {
    written::forget();
    draw(cx);
    let labels = written::labels();
    let unnamed: Vec<&str> = labels
        .iter()
        .filter(|(_, name)| name.as_deref().is_none_or(str::is_empty))
        .map(|(id, _)| id.as_str())
        .collect();
    assert!(
        unnamed.is_empty(),
        "{screen}: labels without a name: {unnamed:?}"
    );
    for id in expected {
        assert!(
            labels.iter().any(|(written, _)| written == id),
            "{screen}: {id} is not read as static text among {:?}",
            labels.iter().map(|(id, _)| id).collect::<Vec<_>>()
        );
    }
}

/// The Stashes dialog reads its title, its "Saved work" header, the
/// detail's prompt and, with a stash selected, the stash's name, identity,
/// file prompt and restore note by their text: each was drawn as plain text
/// AT-SPI reached with no name, the title named only the dialog.
#[gpui::test]
async fn the_stashes_lines_are_named(cx: &mut TestAppContext) {
    let (_fixture, app, cx, root) = window(cx, &history_script(1, 1)).await;
    fixture_git(&root, &["config", "user.name", "Stash Fixture"]);
    fixture_git(&root, &["config", "user.email", "stash@example.invalid"]);
    std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
    fixture_git(&root, &["stash", "push", "-m", "kept for later"]);
    let stashes = open_stashes(&app, cx);
    assert!(
        cx.debug_bounds("saved-stash-row-0").is_some(),
        "the stash is listed"
    );
    labels_named(
        cx,
        "Stashes",
        &[
            "stashes-title",
            "stash-list-heading",
            "stash-detail-placeholder",
        ],
    );
    let row = cx.debug_bounds("saved-stash-row-0").unwrap();
    cx.simulate_click(row.center(), Modifiers::none());
    StashBrowser::lands(&stashes, cx);
    assert!(
        cx.debug_bounds("stash-detail-name").is_some(),
        "the stash's inspection is drawn"
    );
    labels_named(
        cx,
        "A selected stash",
        &[
            "stash-detail-name",
            "stash-detail-identity",
            "stash-file-prompt",
            "stash-restore-note",
        ],
    );
}

/// History, Changes, Tags, the ignore form and Blame: every label each
/// writes is named, Tags', the ignore form's and Blame's lines among them,
/// which were labels named only by their label property.
#[gpui::test]
async fn every_label_of_these_screens_is_named(cx: &mut TestAppContext) {
    let (_fixture, app, cx, root) = window(cx, &history_script(2, 3)).await;
    labels_named(cx, "History", &["repository-heading"]);

    let file = app.read_with(cx, |app, _| {
        app.files
            .iter()
            .position(|file| file.path() == std::path::Path::new("notes.txt"))
            .expect("the commit changes notes.txt")
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(file, window, cx)));
    crate::blame::lands(&app, cx);
    assert!(cx.debug_bounds("blame-heading").is_some(), "Blame is drawn");
    labels_named(
        cx,
        "Blame",
        &[
            "blame-heading",
            "blame-source-scope",
            "blame-lineage-limit",
            "blame-selected-summary",
            "blame-selected-details",
        ],
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
    settle(&app, cx).await;

    std::fs::write(root.join("scratch.txt"), "scratch\n").unwrap();
    refresh(&app, cx).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.working_filter
                .update(cx, |input, cx| input.set_value("no-such-path", window, cx));
            app.filter_working_paths(window, cx);
        })
    });
    crate::path_filter::filters_land(&app, cx);
    assert!(
        cx.debug_bounds("working-no-match").is_some(),
        "the working filter matches nothing"
    );
    labels_named(cx, "Changes", &["working-no-match-line"]);

    open_tags(&app, cx).await;
    labels_named(cx, "Tags", &["tags-dialog-title", "tag-list-empty-line"]);
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx);

    let untracked = app.read_with(cx, |app, _| {
        app.work_status
            .as_ref()
            .and_then(|status| status.entries.iter().find(|entry| entry.untracked).cloned())
            .expect("scratch.txt is untracked")
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_ignore(untracked, window, cx)));
    // The form opens once Ignore…'s review of the file lands.
    crate::ignore::tests::ignore_lands(&app, cx);
    labels_named(
        cx,
        "Ignore",
        &[
            "ignore-dialog-title",
            "ignore-selected-path",
            "ignore-destination-label",
            "ignore-destination-help",
            "ignore-tracked-explanation",
        ],
    );
}

/// A Changes path filter with no match is read where the list draws it, by
/// its whole line, at 18 pt in a half tile too, where it draws "No matches".
#[gpui::test]
async fn a_working_filter_with_no_match_is_read_by_its_line(cx: &mut TestAppContext) {
    let (_fixture, app, cx, root) = window(cx, &history_script(1, 1)).await;
    std::fs::write(root.join("notes.txt"), "changed\n").unwrap();
    std::fs::write(root.join("other.txt"), "other\n").unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.working_filter
                .update(cx, |input, cx| input.set_value("zzz", window, cx));
            app.filter_working_paths(window, cx);
        })
    });
    crate::path_filter::filters_land(&app, cx);
    read(
        cx,
        "A path filter with no match",
        &[
            ("branch-edge", "No remote"),
            ("working-no-match", crate::workspace::WORKING_NO_MATCH),
        ],
        &[],
    );
}

/// The command palette's, Quick Open's and Keyboard Shortcuts' lines and the
/// Projects page's are static text, each named by what it draws: AT-SPI
/// reached none of the palette's and Quick Open's key hints, Quick Open's
/// "18 matches · …" line and note, Keyboard Shortcuts' key hint, the
/// Projects page's "You can also open…" line, its feature notes or its
/// breadcrumb. Keyboard Shortcuts' count is a status named by the line it
/// draws, where it was named "61 matching shortcuts" beside "61 shortcuts ·
/// Ctrl, Alt and Shift". A styled dialog title is named by its value alone,
/// with no label property for macOS to read as a second title.
#[gpui::test]
async fn dialog_hints_and_the_projects_page_are_read_by_their_lines(cx: &mut TestAppContext) {
    let (_fixture, app, cx, _root) = window(cx, &history_script(2, 3)).await;
    let no_second_title = |title: &str| {
        let node = written::get(title).unwrap_or_else(|| panic!("{title} is written"));
        assert_eq!(node.node.label(), None, "{title} has no label property");
    };

    cx.update(|window, cx| app.update(cx, |app, cx| app.open_command_palette(window, cx)));
    draw(cx);
    labels_named(
        cx,
        "Command palette",
        &["command-palette-scope", "command-palette-help"],
    );
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx);

    let quick = cx
        .update(|window, cx| app.update(cx, |app, cx| app.open_quick_form(window, cx)))
        .expect("Quick Open opens");
    crate::revision_inspection::QuickForm::lands(&quick, cx);
    draw(cx);
    assert!(
        written::get("quick-open-help")
            .and_then(|line| line.node.value().map(|value| value.contains(" match")))
            .unwrap_or(false),
        "Quick Open counts its paths"
    );
    labels_named(
        cx,
        "Quick Open",
        &["quick-open-title", "quick-open-help", "quick-open-note"],
    );
    no_second_title("quick-open-title");
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx);

    cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
    draw(cx);
    labels_named(
        cx,
        "Keyboard Shortcuts",
        &["keyboard-shortcuts-title", "shortcut-help-keys"],
    );
    no_second_title("keyboard-shortcuts-title");
    let count = written::get("shortcut-help-count").expect("the count is written");
    assert_eq!(count.node.role(), Role::Status);
    assert_eq!(
        count.node.label(),
        Some(crate::shortcuts::count_line(crate::shortcuts::listed_shortcuts()).as_str()),
        "the count is named by the line it draws"
    );
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx);

    cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
    draw(cx);
    labels_named(
        cx,
        "Projects",
        &[
            "project-hub-app",
            "project-hub-page",
            "hub-open-recent-hint",
            "hub-open-feature-clock-title",
            "hub-open-feature-clock-detail",
            "hub-open-feature-changes-title",
            "hub-open-feature-changes-detail",
        ],
    );
}
