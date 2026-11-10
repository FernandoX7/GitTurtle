use super::*;
use crate::{AppPage, activity, preferences::AppSettings, recovery_drafts, repository_tabs};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::{cell::RefCell, fs, path::Path, rc::Rc};

fn test_app(
    cx: &mut TestAppContext,
    settings: AppSettings,
    library: ProjectLibrary,
) -> (Entity<GitTurtle>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    crate::views::tests::init_app(cx);
    let preferences = Preferences {
        settings,
        project_library: library,
        ..Default::default()
    };
    let captured = Rc::new(RefCell::new(None));
    let observed = captured.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| {
            GitTurtle::new(
                None,
                preferences.clone(),
                repository_tabs::Session::default(),
                activity::State::default(),
                recovery_drafts::State::default(),
                window,
                cx,
            )
        });
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = observed.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1200.), px(720.)));
    // Without a repository the application opens the hub, which lists projects
    // itself. Settings is the nearest page that presents the pane.
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(cx);
    (app, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
}

async fn settle_save(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let barrier = app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())));
    barrier.await.unwrap().unwrap();
    settle(cx);
}

fn library_with_group() -> (ProjectLibrary, u32, Vec<PathBuf>) {
    let paths: Vec<PathBuf> = ["alpha", "beta"]
        .iter()
        .map(|name| Path::new("/projects").join(name))
        .collect();
    let mut library = ProjectLibrary::default();
    for path in &paths {
        library.remember(path);
    }
    let group = library.create_group(None, "Work").unwrap();
    library.move_project(&paths[0], Some(group)).unwrap();
    (library, group, paths)
}

fn enabled() -> AppSettings {
    AppSettings {
        project_pane: true,
        ..Default::default()
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn rows_sort_each_level_by_name_and_tell_identically_named_projects_apart() {
    let mut library = ProjectLibrary::default();
    for path in ["/work/api", "/home/api", "/work/Zebra", "/work/beta"] {
        assert!(library.remember(Path::new(path)));
    }
    let tools = library.create_group(None, "tools").unwrap();
    let archive = library.create_group(None, "Archive").unwrap();
    library
        .move_project(Path::new("/home/api"), Some(tools))
        .unwrap();
    let rows = present_rows(&library, folder_name);
    let summary: Vec<String> = rows
        .iter()
        .map(|row| match row {
            PaneRow::Group { name, depth, .. } => format!("{depth}:{name}/"),
            PaneRow::Project {
                name,
                detail,
                depth,
                ..
            } => format!(
                "{depth}:{name}{}",
                detail
                    .as_ref()
                    .map(|detail| format!(" ({detail})"))
                    .unwrap_or_default()
            ),
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            "0:Archive/",
            "0:tools/",
            "1:api (home)",
            "0:api (work)",
            "0:beta",
            "0:Zebra",
        ]
    );
    // A chosen name that differs removes the ambiguity, so the folder goes.
    let rows = present_rows(&library, |path| {
        if path == Path::new("/home/api") {
            "Home API".into()
        } else {
            folder_name(path)
        }
    });
    assert!(rows.iter().all(|row| !matches!(
        row,
        PaneRow::Project {
            detail: Some(_),
            ..
        }
    )));
    let _ = archive;
}

#[test]
fn move_destinations_exclude_impossible_targets_and_mark_the_current_place() {
    let mut library = ProjectLibrary::default();
    let work = library.create_group(None, "Work").unwrap();
    let clients = library.create_group(Some(work), "Clients").unwrap();
    let deep = library.create_group(Some(clients), "Deep").unwrap();
    let other = library.create_group(None, "Other").unwrap();
    let project = Path::new("/projects/alpha");
    library.remember(project);
    library.move_project(project, Some(clients)).unwrap();

    let labels = |destinations: &[Destination]| {
        destinations
            .iter()
            .map(|d| {
                format!(
                    "{}{}{}",
                    d.label,
                    if d.current { " ✓" } else { "" },
                    if d.allowed { "" } else { " ✗" }
                )
            })
            .collect::<Vec<_>>()
    };
    // A group never sees itself or its own groups; its parent is marked.
    assert_eq!(
        labels(&move_destinations(&library, &RowKey::Group(clients))),
        vec!["Top level", "Other", "Work ✓"]
    );
    assert_eq!(
        labels(&move_destinations(&library, &RowKey::Group(work))),
        vec!["Top level ✓", "Other"]
    );
    // Work is three levels tall, so it fits under a top-level group or its
    // child, but a third level down would push Deep past the limit.
    let mut tall = library.clone();
    let sub = tall.create_group(Some(other), "Sub").unwrap();
    let deeper = tall.create_group(Some(sub), "Deeper").unwrap();
    assert_eq!(
        labels(&move_destinations(&tall, &RowKey::Group(work))),
        vec![
            "Top level ✓",
            "Other",
            "Other › Sub",
            "Other › Sub › Deeper ✗",
        ]
    );
    // A single-level group still fits anywhere the limit allows.
    assert_eq!(
        labels(&move_destinations(&tall, &RowKey::Group(deep))),
        vec![
            "Top level",
            "Other",
            "Other › Sub",
            "Other › Sub › Deeper",
            "Work",
            "Work › Clients ✓",
        ]
    );
    let _ = deeper;
    // A project can go anywhere; its current group is marked.
    assert_eq!(
        labels(&move_destinations(
            &library,
            &RowKey::Project(project.into())
        )),
        vec![
            "Top level",
            "Other",
            "Work",
            "Work › Clients ✓",
            "Work › Clients › Deep"
        ]
    );
}

#[gpui::test]
fn the_pane_appears_only_when_it_is_enabled_and_a_page_needs_it(cx: &mut TestAppContext) {
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, AppSettings::default(), library.clone());
    app.read_with(cx, |app, _| assert_eq!(app.page, AppPage::Settings));
    assert!(cx.debug_bounds("project-pane").is_none());

    // The View menu and command palette toggle the same saved switch.
    cx.update(|window, cx| app.update(cx, |app, cx| app.toggle_project_pane(window, cx)));
    settle(cx);
    let pane = cx.debug_bounds("project-pane").expect("enabled pane");
    assert!(pane.size.width >= px(239.));
    // The pane sits at the far left, under the tab strip and header.
    assert!(pane.origin.x < px(1.));
    app.read_with(cx, |app, _| assert!(app.settings.project_pane));

    // The minimum 1000 px window keeps 800 px for the page; the saved width
    // stays 240 for larger windows.
    cx.simulate_resize(size(px(1000.), px(680.)));
    settle(cx);
    let pane = cx
        .debug_bounds("project-pane")
        .expect("pane at the minimum window");
    assert!(pane.size.width <= px(201.) && pane.size.width >= px(180.));
    app.read_with(cx, |app, _| {
        assert_eq!(app.settings.project_pane_width, 240.)
    });
    cx.simulate_resize(size(px(1200.), px(720.)));
    settle(cx);

    // The project hub already lists projects across the whole window.
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
    settle(cx);
    app.read_with(cx, |app, _| assert_eq!(app.page, AppPage::Projects));
    assert!(cx.debug_bounds("project-pane").is_none());
}

#[gpui::test]
async fn collapsing_a_group_hides_its_projects_and_outlives_a_restart(cx: &mut TestAppContext) {
    let (library, group, paths) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    assert!(cx.debug_bounds("project-pane-row-0").is_some());
    assert!(cx.debug_bounds("project-pane-row-1").is_some());
    assert!(cx.debug_bounds("project-pane-row-2").is_some());

    cx.update(|window, cx| app.update(cx, |app, cx| app.toggle_project_group(group, window, cx)));
    settle(cx);
    // The group and the ungrouped project remain; the grouped project is hidden.
    assert!(cx.debug_bounds("project-pane-row-1").is_some());
    assert!(cx.debug_bounds("project-pane-row-2").is_none());

    settle_save(&app, cx).await;
    let saved = Preferences::load();
    assert_eq!(
        saved.project_library.group(group).map(|g| g.collapsed),
        Some(true)
    );
    assert!(saved.project_library.contains_project(&paths[0]));
    app.read_with(cx, |app, _| {
        assert_eq!(app.project_pane.pending_saves(), 0);
        assert!(app.project_pane.error.is_none());
    });
}

#[gpui::test]
async fn the_keyboard_browses_the_tree_like_the_navigator(cx: &mut TestAppContext) {
    let (library, group, paths) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    let cursor = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| app.project_pane.cursor)
    };

    // Down enters the tree at its first row, the group.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.move_project_cursor(true, false, window, cx)
        })
    });
    assert_eq!(cursor(&app, cx), Some(0));
    app.read_with(cx, |app, _| {
        assert!(matches!(app.project_pane.rows[0], PaneRow::Group { .. }));
    });

    // Left collapses the group; the cursor stays on it.
    cx.update(|window, cx| app.update(cx, |app, cx| app.expand_project_row(false, window, cx)));
    settle(cx);
    assert_eq!(cursor(&app, cx), Some(0));
    assert!(cx.debug_bounds("project-pane-row-2").is_none());

    // Right expands it again, then Right steps into it.
    cx.update(|window, cx| app.update(cx, |app, cx| app.expand_project_row(true, window, cx)));
    settle(cx);
    assert!(cx.debug_bounds("project-pane-row-2").is_some());
    cx.update(|window, cx| app.update(cx, |app, cx| app.expand_project_row(true, window, cx)));
    assert_eq!(cursor(&app, cx), Some(1));
    app.read_with(cx, |app, _| {
        assert!(matches!(
            &app.project_pane.rows[1],
            PaneRow::Project { path, depth: 1, .. } if **path == paths[0]
        ));
    });

    // Left on a nested project returns to its group; End reaches the last row.
    cx.update(|window, cx| app.update(cx, |app, cx| app.expand_project_row(false, window, cx)));
    assert_eq!(cursor(&app, cx), Some(0));
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.move_project_cursor(true, true, window, cx)
        })
    });
    assert_eq!(cursor(&app, cx), Some(2));

    // Shift-F10 opens the cursor row's actions and Escape closes them.
    cx.update(|window, cx| app.update(cx, |app, cx| app.manage_project_row(window, cx)));
    settle(cx);
    app.read_with(cx, |app, _| {
        assert!(app.project_pane.keyboard_menu.is_some())
    });
    let overlay = cx
        .debug_bounds("project-pane-menu-overlay")
        .expect("keyboard menu overlay renders");
    assert!(overlay.size.width >= px(1200.));
    cx.update(|window, cx| app.update(cx, |app, cx| app.dismiss_project_menu(window, cx)));
    settle(cx);
    app.read_with(cx, |app, _| {
        assert!(app.project_pane.keyboard_menu.is_none());
    });

    // Removing the group keeps the cursor on the same project row.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.project_pane.cursor = Some(1);
            app.edit_project_library(
                |library| {
                    library.remove_group(group);
                    Ok(())
                },
                window,
                cx,
            )
        })
    });
    settle_save(&app, cx).await;
    app.read_with(cx, |app, _| {
        let index = app.project_pane.cursor.expect("cursor follows the project");
        assert!(matches!(
            &app.project_pane.rows[index],
            PaneRow::Project { path, depth: 0, .. } if **path == paths[0]
        ));
    });
}

/// Shift-F10 and the Menu key on the tree's cursor row open its menu, the
/// one a right-click opens, and Escape returns focus to the tree. The Menu
/// key did nothing.
#[gpui::test]
fn shift_f10_and_the_menu_key_open_the_cursor_rows_menu(cx: &mut TestAppContext) {
    const MENU_KEY: &str = if cfg!(target_os = "macos") {
        "shift-f10"
    } else {
        "menu"
    };
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|_, cx| native_accessibility::bind_keys(cx));
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.move_project_cursor(true, false, window, cx);
            window.focus(&app.project_pane.focus, cx);
        })
    });
    settle(cx);
    let focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| app.read(cx).project_pane.focus.is_focused(window))
    };
    for key in ["shift-f10", MENU_KEY] {
        cx.simulate_keystrokes(key);
        settle(cx);
        assert!(
            cx.debug_bounds("menu-item: Work · Label").is_some(),
            "{key} opened the group's menu"
        );
        assert!(!focused(cx), "{key}: the menu takes the keys");
        cx.simulate_keystrokes("escape");
        settle(cx);
        assert!(
            cx.debug_bounds("menu-item: Work · Label").is_none(),
            "{key}: Escape closed it"
        );
        assert!(focused(cx), "{key}: Escape returned focus to the tree");
    }
}

/// A project row's menu opened by Shift-F10 or the Menu key follows writes
/// as its right-click menu does: Open project is disabled in place as a
/// write starts and available again as it ends, with focus kept in the menu.
/// It read the write once, as it opened, and kept Open project so. Its
/// keyboard's cursor, on Open project as it opens, stays on Open project
/// while the write refuses it and Open project states what it waits for,
/// and is on it, able to act, once the write ends: moved to the nearest item
/// that could act, it rested on Rename project… and stayed there after the
/// write, so Enter renamed instead of opening; rested on no item, it showed
/// nothing of where the person had put it.
#[gpui::test]
fn a_project_rows_keyboard_menu_follows_writes(cx: &mut TestAppContext) {
    const MENU_KEY: &str = if cfg!(target_os = "macos") {
        "shift-f10"
    } else {
        "menu"
    };
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|_, cx| native_accessibility::bind_keys(cx));
    // The last row, beta, a project.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.move_project_cursor(true, true, window, cx);
            window.focus(&app.project_pane.focus, cx);
        })
    });
    settle(cx);
    let in_menu = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            app.read(cx)
                .project_pane
                .keyboard_menu
                .as_ref()
                .is_some_and(|menu| menu.has_focus(window, cx))
        })
    };
    let open = |cx: &mut VisualTestContext, disabled: bool| {
        let selector = if disabled {
            "menu-item: Open project, available when testing finishes · MenuItem disabled"
        } else {
            "menu-item: Open project · MenuItem"
        };
        cx.debug_bounds(selector).is_some()
    };
    let busy = |cx: &mut VisualTestContext, busy: Option<&'static str>| {
        app.update(cx, |app, cx| {
            app.operation_busy = busy;
            cx.notify();
        });
        settle(cx);
    };
    let cursor = |cx: &mut VisualTestContext| {
        crate::native_accessibility::menu_tests::cursor_among(
            cx,
            &[
                "Open project",
                "Open project, available when testing finishes",
                "Rename project…",
            ],
        )
    };
    for key in ["shift-f10", MENU_KEY] {
        cx.simulate_keystrokes(key);
        settle(cx);
        assert!(open(cx, false), "{key}: Open project at rest");
        assert_eq!(
            cursor(cx),
            ["Open project"],
            "{key}: the cursor as it opens"
        );
        busy(cx, Some("Testing"));
        assert!(open(cx, true), "{key}: Open project once a write starts");
        assert!(
            in_menu(cx),
            "{key}: focus stays in the menu as a write starts"
        );
        assert_eq!(
            cursor(cx),
            ["Open project, available when testing finishes"],
            "{key}: the cursor stays on Open project while it waits for the write"
        );
        busy(cx, None);
        assert!(open(cx, false), "{key}: Open project once the write ends");
        assert!(in_menu(cx), "{key}: focus stays in the menu as it ends");
        assert_eq!(
            cursor(cx),
            ["Open project"],
            "{key}: the cursor back on Open project once the write ends"
        );
        cx.simulate_keystrokes("escape");
        settle(cx);
        assert!(
            app.read_with(cx, |app, _| app.project_pane.keyboard_menu.is_none()),
            "{key}: Escape closed the menu"
        );
    }
}

/// A project row's menu says why a running write refuses Open project, as
/// every menu that refuses items for a write does
/// (`native_accessibility::waiting_item`): during a Fetch it reads "Open
/// project, available when fetching remote finishes", with the Fetch's label
/// at the row's end in secondary text, and Rename project…, which no write
/// refuses, stays a plain available item. It refused Open project with
/// nothing to say why.
#[gpui::test]
fn a_project_rows_menu_names_the_write_open_project_waits_for(cx: &mut TestAppContext) {
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|_, cx| native_accessibility::bind_keys(cx));
    // The last row, beta, a project.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.move_project_cursor(true, true, window, cx);
            window.focus(&app.project_pane.focus, cx);
        })
    });
    settle(cx);
    cx.simulate_keystrokes("shift-f10");
    settle(cx);
    assert!(
        cx.debug_bounds("menu-item: Open project · MenuItem")
            .is_some(),
        "Open project at rest"
    );
    app.update(cx, |app, cx| {
        app.operation_busy = Some("Fetching remote…");
        cx.notify();
    });
    settle(cx);
    let muted = cx
        .update(|_, cx| gpui::Rgba::from(gpui_kit::component::Theme::global(cx).muted_foreground));
    let row = cx
        .debug_bounds(
            "menu-item: Open project, available when fetching remote finishes · MenuItem disabled",
        )
        .expect("Open project names the Fetch it waits for");
    let command = cx
        .debug_bounds("menu-command: Open project")
        .expect("Open project's command");
    let detail = cx
        .debug_bounds(format!("menu-reason: Open project · Fetching remote… · {muted:?}").leak())
        .expect("the Fetch's label in secondary text");
    assert!(
        command.right() < detail.left() && detail.right() <= row.right(),
        "the reason {detail:?} after the command {command:?} in {row:?}"
    );
    assert!(
        cx.debug_bounds("menu-item: Rename project… · MenuItem")
            .is_some(),
        "Rename project… stays available"
    );
    app.update(cx, |app, cx| {
        app.operation_busy = None;
        cx.notify();
    });
    settle(cx);
    assert!(
        cx.debug_bounds("menu-item: Open project · MenuItem")
            .is_some(),
        "Open project once the write ends"
    );
    cx.simulate_keystrokes("escape");
    settle(cx);
}

/// The window's polite announcements, each by its node and its text: the
/// node a fresh one for each announcement, so one heard again shows as a
/// new node.
fn polite_announcements(cx: &mut VisualTestContext) -> Option<(ElementId, String)> {
    let announced = crate::copy_feedback::tests::announced(cx)?;
    let live = native_accessibility::written::announced_from("copy-announcement");
    assert!(
        !live.is_empty()
            && live
                .iter()
                .all(|(_, how)| how.live == gpui::accesskit::Live::Polite),
        "announced politely, not as an alert: {live:?}"
    );
    Some(announced)
}

/// A project row's keyboard menu whose cursor a write leaves resting, Open
/// project refused under it, says once, politely, why Open project waits:
/// "Open project, available when fetching remote finishes". Built again
/// while the rest goes on, as another write's label replaces the Fetch's, it
/// says nothing more; resting again after the write, it says why again. A
/// menu the pointer owns says nothing. The cursor rested on no item with
/// nothing to say why Enter then did nothing.
#[gpui::test]
fn a_resting_menu_cursor_says_once_why_its_item_waits(cx: &mut TestAppContext) {
    const WAITS: &str = "Open project, available when fetching remote finishes";
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|_, cx| native_accessibility::bind_keys(cx));
    let busy = |cx: &mut VisualTestContext, busy: Option<&'static str>| {
        app.update(cx, |app, cx| {
            app.operation_busy = busy;
            cx.notify();
        });
        settle(cx);
    };
    // The last row, beta, a project.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.move_project_cursor(true, true, window, cx);
            window.focus(&app.project_pane.focus, cx);
        })
    });
    settle(cx);
    cx.simulate_keystrokes("shift-f10");
    settle(cx);
    assert_eq!(
        crate::native_accessibility::menu_tests::cursor_among(cx, &["Open project"]),
        ["Open project"],
        "the cursor on Open project as the menu opens"
    );
    assert_eq!(polite_announcements(cx), None, "nothing said at rest");

    busy(cx, Some("Fetching remote…"));
    let (first, said) = polite_announcements(cx).expect("the rest is announced");
    assert_eq!(said, WAITS, "it says why Open project waits");
    busy(cx, Some("Pulling fast-forward…"));
    assert_eq!(
        polite_announcements(cx),
        Some((first.clone(), WAITS.to_string())),
        "built again while the rest goes on, it says nothing more"
    );
    busy(cx, None);
    assert_eq!(
        crate::native_accessibility::menu_tests::cursor_among(cx, &["Open project"]),
        ["Open project"],
        "the cursor back on Open project once the write ends"
    );
    busy(cx, Some("Fetching remote…"));
    let (again, said) = polite_announcements(cx).expect("the new rest is announced");
    assert!(
        again != first && said == WAITS,
        "resting again, it says why again: {again:?} {said}"
    );
    busy(cx, None);
    cx.simulate_keystrokes("escape");
    settle(cx);

    // The actions button's menu, its selection the pointer's.
    let beta = app.read_with(cx, |app, _| app.project_pane.rows.len() - 1);
    let button = cx
        .debug_bounds(format!("project-row-menu-{beta}").leak())
        .expect("beta's actions button");
    cx.simulate_mouse_move(button.center(), None, Modifiers::default());
    settle(cx);
    cx.simulate_click(button.center(), Modifiers::default());
    settle(cx);
    let open = cx
        .debug_bounds("menu-item: Open project · MenuItem")
        .expect("Open project at rest");
    cx.simulate_mouse_move(open.center(), None, Modifiers::default());
    settle(cx);
    busy(cx, Some("Fetching remote…"));
    assert_eq!(
        polite_announcements(cx).map(|(node, _)| node),
        Some(again),
        "the pointer's menu says nothing"
    );
    busy(cx, None);
    cx.simulate_keystrokes("escape");
    settle(cx);
}

/// A project row's actions button, the control a hover reveals at the row's
/// end, opens the menu its right-click and Shift-F10 open, and it follows
/// writes as they do: Open project, which a write refuses, is disabled in
/// place as a write starts and available again as it ends, the menu open
/// throughout. Built only as it opened, it kept Open project as the write
/// found it, available while the write ran, until it was opened again.
#[gpui::test]
fn a_project_rows_actions_button_menu_follows_writes(cx: &mut TestAppContext) {
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    // The last row, beta, a project.
    let beta = app.read_with(cx, |app, _| app.project_pane.rows.len() - 1);
    let button = cx
        .debug_bounds(format!("project-row-menu-{beta}").leak())
        .expect("beta's actions button");
    cx.simulate_mouse_move(button.center(), None, Modifiers::default());
    settle(cx);
    cx.simulate_click(button.center(), Modifiers::default());
    settle(cx);
    let open = |cx: &mut VisualTestContext, disabled: bool| {
        let selector = if disabled {
            "menu-item: Open project, available when testing finishes · MenuItem disabled"
        } else {
            "menu-item: Open project · MenuItem"
        };
        cx.debug_bounds(selector).is_some()
    };
    let busy = |cx: &mut VisualTestContext, busy: Option<&'static str>| {
        app.update(cx, |app, cx| {
            app.operation_busy = busy;
            cx.notify();
        });
        settle(cx);
    };
    assert!(open(cx, false), "the button opened Open project at rest");
    assert_eq!(
        app.read_with(cx, |app, _| app.project_pane.menu_open.clone()),
        app.read_with(cx, |app, _| Some(app.project_pane.rows[beta].key())),
        "beta's menu is open"
    );
    busy(cx, Some("Testing"));
    assert!(open(cx, true), "Open project once a write starts");
    assert!(!open(cx, false), "and offered no longer");
    busy(cx, None);
    assert!(open(cx, false), "Open project once the write ends");
    cx.simulate_keystrokes("escape");
    settle(cx);
}

/// A project row's keyboard menu stays with its project as a project removed
/// above it moves the rows up, drawn below the project's row where it now
/// stands, and closes, with focus back on the tree, once its own project
/// leaves the list. It once closed as its row moved, and before that, built
/// again for the place it opened beside, it would have acted on a project
/// that was no longer there.
#[gpui::test]
async fn a_project_rows_keyboard_menu_follows_its_row_and_closes_with_it(cx: &mut TestAppContext) {
    let (library, _, paths) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|_, cx| native_accessibility::bind_keys(cx));
    // The last row, beta, a project, below alpha in the group.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.move_project_cursor(true, true, window, cx);
            window.focus(&app.project_pane.focus, cx);
        })
    });
    settle(cx);
    cx.simulate_keystrokes("shift-f10");
    settle(cx);
    assert!(
        cx.debug_bounds("menu-item: Open project · MenuItem")
            .is_some(),
        "beta's menu opened"
    );
    let forget = |cx: &mut VisualTestContext, path: PathBuf| {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.edit_project_library(
                    |library| {
                        library.forget_project(&path);
                        Ok(())
                    },
                    window,
                    cx,
                )
            })
        });
        settle(cx);
    };
    forget(cx, paths[0].clone());
    assert!(
        app.read_with(cx, |app, _| app.project_pane.keyboard_menu.is_some()),
        "the menu stays open on beta as a row above it goes"
    );
    let beta = app.read_with(cx, |app, _| app.project_pane.cursor.expect("on beta"));
    let row = cx
        .debug_bounds(format!("project-pane-row-{beta}").leak())
        .expect("beta's row");
    let item = cx
        .debug_bounds("menu-item: Open project · MenuItem")
        .expect("the menu's item");
    assert!(
        item.top() >= row.bottom(),
        "the menu {item:?} lies below beta's row {row:?} where it now stands"
    );
    forget(cx, paths[1].clone());
    assert!(
        app.read_with(cx, |app, _| app.project_pane.keyboard_menu.is_none()),
        "the menu closed with its project"
    );
    assert!(
        cx.update(|window, cx| app.read(cx).project_pane.focus.is_focused(window)),
        "focus is back on the tree"
    );
    settle_save(&app, cx).await;
}

/// A row's menu opens with the row's name, a label. Opened from the
/// keyboard, it starts on the first action below the name, so Enter alone
/// renames the group: the kit's first Down used to select the name, and
/// Enter did nothing, and a keyboard's menu highlighted nothing until Down.
#[gpui::test]
fn a_keyboard_row_menu_starts_on_its_first_action(cx: &mut TestAppContext) {
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.move_project_cursor(true, false, window, cx)
        })
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.manage_project_row(window, cx)));
    settle(cx);
    assert!(
        cx.debug_bounds("menu-item: Work · Label").is_some(),
        "the menu opens with the group's name"
    );
    // Move to titles the destinations, and the group's current place is
    // checked and unavailable: moving it there would do nothing.
    for entry in [
        "menu-item: Move to · Label",
        "menu-item: Top level · MenuItemCheckBox True disabled",
    ] {
        assert!(cx.debug_bounds(entry).is_some(), "{entry}");
    }
    cx.simulate_keystrokes("enter");
    settle(cx);
    app.read_with(cx, |app, _| {
        assert!(app.project_pane.keyboard_menu.is_none(), "the menu closed");
        assert!(
            app.project_pane.group_form.is_some(),
            "Rename group… opened its form"
        );
    });
}

/// A dialog opened from a row's menu button, which is no Tab stop and takes
/// no focus itself, gives focus back to the tree as a keyboard's row menu
/// does, and the next arrow moves the cursor. It left focus nowhere, and no
/// key reached anything until a click.
#[gpui::test]
fn a_dialog_from_a_rows_menu_button_returns_focus_to_the_tree(cx: &mut TestAppContext) {
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|_, cx| native_accessibility::bind_keys(cx));
    settle(cx);
    let button = cx
        .debug_bounds("project-row-menu-0")
        .expect("the group's actions button");
    cx.simulate_mouse_move(button.center(), None, Modifiers::default());
    settle(cx);
    cx.simulate_click(button.center(), Modifiers::default());
    settle(cx);
    let item = cx
        .debug_bounds("menu-item: Rename group… · MenuItem")
        .expect("the menu offers Rename group…");
    cx.simulate_click(item.center(), Modifiers::default());
    settle(cx);
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "Rename group… opened its form"
    );
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "Escape closes the form"
    );
    assert!(
        cx.update(|window, cx| app.read(cx).project_pane.focus.is_focused(window)),
        "focus is on the tree"
    );
    let before = app.read_with(cx, |app, _| app.project_pane.cursor);
    cx.simulate_keystrokes("down");
    settle(cx);
    assert_ne!(
        app.read_with(cx, |app, _| app.project_pane.cursor),
        before,
        "the next arrow moves the cursor"
    );
}

#[gpui::test]
fn an_older_save_reply_never_replaces_a_newer_edit(cx: &mut TestAppContext) {
    let (library, group, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library.clone());
    let mut newer = library.clone();
    newer.rename_group(group, "Newer").unwrap();
    let older = Preferences {
        project_library: library.clone(),
        ..Default::default()
    };
    let newest = Preferences {
        project_library: newer.clone(),
        ..Default::default()
    };
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            let first = app.begin_project_library_save();
            app.set_project_library(newer.clone());
            let second = app.begin_project_library_save();
            // The first reply is for a write that preceded the rename.
            app.finish_project_library_save(first, &Ok(older.clone()), cx);
            assert_eq!(app.project_library, newer);
            assert_eq!(app.project_pane.pending_saves(), 1);
            app.finish_project_library_save(second, &Ok(newest.clone()), cx);
            assert_eq!(app.project_library, newer);
            assert_eq!(app.project_pane.pending_saves(), 0);

            // Each reply wakes its own task, so older ones can be handled
            // last: an older save's success or failure still cannot undo the
            // rename or report a stale outcome.
            app.set_project_library(library.clone());
            let first = app.begin_project_library_save();
            app.set_project_library(newer.clone());
            let second = app.begin_project_library_save();
            app.finish_project_library_save(second, &Ok(newest), cx);
            app.finish_project_library_save(first, &Ok(older), cx);
            assert_eq!(app.project_library, newer);
            let first = app.begin_project_library_save();
            let second = app.begin_project_library_save();
            app.finish_project_library_save(
                second,
                &Ok(Preferences {
                    project_library: newer.clone(),
                    ..Default::default()
                }),
                cx,
            );
            app.finish_project_library_save(
                first,
                &Err(anyhow::anyhow!("an older write failed")),
                cx,
            );
            assert_eq!(app.project_library, newer);
            assert!(app.project_pane.error.is_none());
            // Nor does an older success clear the newest save's failure.
            let first = app.begin_project_library_save();
            let second = app.begin_project_library_save();
            app.finish_project_library_save(
                second,
                &Err(anyhow::anyhow!("the newest write failed")),
                cx,
            );
            app.finish_project_library_save(
                first,
                &Ok(Preferences {
                    project_library: newer.clone(),
                    ..Default::default()
                }),
                cx,
            );
            assert!(app.project_pane.error.is_some());
            assert_eq!(app.project_pane.pending_saves(), 0);
        })
    });
}

/// The reply of a recent-project save that added `opened` to `stored`.
fn remembered(stored: &ProjectLibrary, opened: &Path) -> Preferences {
    let mut project_library = stored.clone();
    project_library.remember(opened);
    Preferences {
        recent_repositories: vec![opened.to_owned()],
        project_library,
        ..Default::default()
    }
}

#[gpui::test]
async fn opening_a_project_during_a_pane_save_keeps_both_in_every_reply_order(
    cx: &mut TestAppContext,
) {
    let (base, group, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), base.clone());
    let opened = Path::new("/projects/gamma");
    let mut edited = base.clone();
    edited.rename_group(group, "Renamed").unwrap();
    let merged = remembered(&edited, opened).project_library;
    let pane_reply = Preferences {
        project_library: edited.clone(),
        ..Default::default()
    };

    for pane_submitted_first in [true, false] {
        for pane_reply_first in [true, false] {
            let case = format!(
                "pane save submitted {}, its reply handled {}",
                if pane_submitted_first {
                    "first"
                } else {
                    "second"
                },
                if pane_reply_first { "first" } else { "second" },
            );
            let resaved = cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.set_project_library(base.clone());
                    // The serialized writer stores each list in submission
                    // order, so the recent save adds the project to whatever
                    // the store held when it ran.
                    let (pane, recent, recent_reply) = if pane_submitted_first {
                        app.set_project_library(edited.clone());
                        let pane = app.begin_project_library_save();
                        let recent = app.begin_recent_project_save(opened);
                        (pane, recent, remembered(&edited, opened))
                    } else {
                        let recent = app.begin_recent_project_save(opened);
                        app.set_project_library(edited.clone());
                        let pane = app.begin_project_library_save();
                        (pane, recent, remembered(&base, opened))
                    };
                    let latest = app.project_pane.latest_save;
                    if pane_reply_first {
                        app.finish_project_library_save(pane, &Ok(pane_reply.clone()), cx);
                        app.absorb_remembered_project(recent, &recent_reply, window, cx);
                    } else {
                        app.absorb_remembered_project(recent, &recent_reply, window, cx);
                        app.finish_project_library_save(pane, &Ok(pane_reply.clone()), cx);
                    }
                    assert_eq!(app.project_library, merged, "{case}");
                    assert!(app.project_pane.error.is_none(), "{case}");
                    // A pane save after the recent one wrote the list without
                    // the project, so only then is the merged list saved.
                    let resaved = app.project_pane.latest_save > latest;
                    assert_eq!(resaved, !pane_submitted_first, "{case}");
                    assert_eq!(app.project_pane.pending_saves(), usize::from(resaved));
                    if resaved {
                        assert_eq!(
                            app.project_pane.latest_pane_save,
                            app.project_pane.latest_save
                        );
                    }
                    resaved
                })
            });
            settle_save(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert_eq!(app.project_library, merged, "{case}");
                assert_eq!(app.project_pane.pending_saves(), 0, "{case}");
                assert!(app.project_pane.error.is_none(), "{case}");
            });
            if resaved {
                assert_eq!(Preferences::load().project_library, merged, "{case}");
            }
        }
    }
}

#[gpui::test]
fn a_recent_reply_after_a_failed_newer_pane_save_leaves_the_write_to_retry(
    cx: &mut TestAppContext,
) {
    let (base, group, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), base.clone());
    let opened = Path::new("/projects/gamma");
    let mut edited = base.clone();
    edited.rename_group(group, "Renamed").unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let recent = app.begin_recent_project_save(opened);
            app.set_project_library(edited.clone());
            let pane = app.begin_project_library_save();
            app.finish_project_library_save(pane, &Err(anyhow::anyhow!("disk full")), cx);
            let latest = app.project_pane.latest_save;
            // The failed pane save wrote nothing, so the store kept the
            // project. The reply merges it beside the unsaved edit and
            // submits nothing: only Retry writes the list again.
            app.absorb_remembered_project(recent, &remembered(&base, opened), window, cx);
            assert_eq!(
                app.project_library,
                remembered(&edited, opened).project_library
            );
            assert!(app.project_pane.error.is_some());
            assert_eq!(app.project_pane.latest_save, latest);
            assert_eq!(app.project_pane.pending_saves(), 0);
        })
    });
}

#[gpui::test]
async fn replies_handled_out_of_order_at_the_bound_save_the_project_the_pane_kept(
    cx: &mut TestAppContext,
) {
    // Grouped projects fill every place but one, so each opened project
    // drops the one ungrouped project before it.
    let mut base = ProjectLibrary::default();
    let group = base.create_group(None, "Work").unwrap();
    for index in 1..crate::project_library::MAX_PROJECTS {
        let path = Path::new("/projects/grouped").join(index.to_string());
        assert!(base.remember(&path));
        base.move_project(&path, Some(group)).unwrap();
    }
    let ungrouped = Path::new("/projects/ungrouped");
    assert!(base.remember(ungrouped));
    let (app, cx) = test_app(cx, enabled(), base.clone());
    let (first, second) = (Path::new("/projects/first"), Path::new("/projects/second"));
    // The writer drops the ungrouped project for the first, then the first
    // for the second.
    let first_reply = remembered(&base, first);
    let second_reply = remembered(&first_reply.project_library, second);
    assert!(!second_reply.project_library.contains_project(first));

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let first_save = app.begin_recent_project_save(first);
            let second_save = app.begin_recent_project_save(second);
            let latest = app.project_pane.latest_save;
            app.absorb_remembered_project(second_save, &second_reply, window, cx);
            app.absorb_remembered_project(first_save, &first_reply, window, cx);
            // The pane dropped the second project for the first, unlike the
            // store, so it writes its own list after both recent saves.
            assert!(app.project_library.contains_project(first));
            assert!(!app.project_library.contains_project(second));
            assert!(!app.project_library.contains_project(ungrouped));
            assert!(app.project_pane.latest_pane_save > latest);
            assert_eq!(
                app.project_pane.latest_pane_save,
                app.project_pane.latest_save
            );
            assert!(app.project_pane.pending_saves() > 0);
        })
    });
    settle_save(&app, cx).await;
    let shown = app.read_with(cx, |app, _| {
        assert_eq!(app.project_pane.pending_saves(), 0);
        assert!(app.project_pane.error.is_none());
        app.project_library.clone()
    });
    assert!(shown.contains_project(first));
    assert_eq!(Preferences::load().project_library, shown);
}

#[gpui::test]
fn a_recent_project_reply_adds_only_what_the_pane_lacked_and_the_store_kept(
    cx: &mut TestAppContext,
) {
    let (library, _, paths) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library.clone());
    let listed = &paths[1];
    let mut removed = library.clone();
    assert!(removed.forget_project(listed));
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            // The pane listed the project when it was reopened, and the user
            // removed it before the recent save's reply.
            let recent = app.begin_recent_project_save(listed);
            app.set_project_library(removed.clone());
            let pane = app.begin_project_library_save();
            app.absorb_remembered_project(recent, &remembered(&library, listed), window, cx);
            assert_eq!(app.project_library, removed);
            assert_eq!(app.project_pane.pending_saves(), 1);
            app.finish_project_library_save(
                pane,
                &Ok(Preferences {
                    project_library: removed.clone(),
                    ..Default::default()
                }),
                cx,
            );
            assert_eq!(app.project_library, removed);
            assert_eq!(app.project_pane.pending_saves(), 0);

            // A store that refused the project, such as a list full of
            // grouped projects, has no addition to merge.
            let refused = Path::new("/projects/gamma");
            let recent = app.begin_recent_project_save(refused);
            app.absorb_remembered_project(
                recent,
                &Preferences {
                    recent_repositories: vec![refused.to_owned()],
                    project_library: removed.clone(),
                    ..Default::default()
                },
                window,
                cx,
            );
            assert_eq!(app.project_library, removed);
            assert_eq!(app.project_pane.pending_saves(), 0);
        })
    });
}

#[gpui::test]
async fn a_failed_save_stays_visible_in_the_pane_until_retry_succeeds(cx: &mut TestAppContext) {
    let (library, group, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    // A directory where the store should be makes every write fail.
    let store = crate::preferences::settings_path().unwrap();
    fs::create_dir_all(&store).unwrap();

    cx.update(|window, cx| app.update(cx, |app, cx| app.toggle_project_group(group, window, cx)));
    settle_save(&app, cx).await;
    assert!(cx.debug_bounds("project-pane-error").is_some());
    // The failure interrupts: it is announced at once, as a whole.
    native_accessibility::written::forget();
    settle(cx);
    assert_eq!(
        native_accessibility::written::announced("project-pane-error"),
        Some(native_accessibility::written::Announced::ASSERTIVE)
    );
    // Its name, which AT-SPI announces, and its value, which macOS does.
    let error = app.read_with(cx, |app, _| app.project_pane.error.clone());
    let alert = native_accessibility::written::get("project-pane-error")
        .expect("the failure's node")
        .node;
    assert_eq!(
        (alert.label(), alert.value()),
        (error.as_deref(), error.as_deref()),
        "the failure is its node's name and value"
    );
    let edited = app.read_with(cx, |app, _| {
        assert!(
            app.project_pane
                .error
                .as_deref()
                .is_some_and(|error| error.starts_with("Could not save the project list"))
        );
        // The edit stays on screen so Retry can write it.
        assert!(app.project_library.group(group).unwrap().collapsed);
        assert!(app.operation_error.is_none());
        app.project_library.clone()
    });

    // Opening a project now saves the recents to a writable store holding
    // neither the edit nor the project. Its reply adds the project beside
    // the unsaved edit and leaves the failure in place.
    fs::remove_dir(&store).unwrap();
    let project = tempfile::tempdir().unwrap();
    let opened = project.path().canonicalize().unwrap();
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.path = Some(project.path().to_owned());
            app.remember_repository(window, cx);
            app.path = None;
        })
    });
    settle_save(&app, cx).await;
    let mut merged = edited;
    assert!(merged.remember(&opened));
    assert!(cx.debug_bounds("project-pane-error").is_some());
    app.read_with(cx, |app, _| {
        assert_eq!(app.project_library, merged);
        assert!(app.project_pane.error.is_some());
        assert_eq!(app.project_pane.pending_saves(), 0);
        assert!(app.operation_error.is_none());
    });

    cx.update(|window, cx| app.update(cx, |app, cx| app.retry_project_library_save(window, cx)));
    settle_save(&app, cx).await;
    assert!(cx.debug_bounds("project-pane-error").is_none());
    // Each draw of the window starts its live regions afresh, so the
    // failure drawn earlier is no longer announced.
    assert_eq!(
        native_accessibility::written::announced("project-pane-error"),
        None
    );
    app.read_with(cx, |app, _| {
        assert!(app.project_pane.error.is_none());
        assert_eq!(app.project_library, merged);
    });
    let saved = Preferences::load();
    assert_eq!(saved.project_library, merged);
    assert_eq!(saved.recent_repositories, vec![opened]);
}

#[gpui::test]
async fn naming_a_group_saves_it_and_removing_it_keeps_the_projects(cx: &mut TestAppContext) {
    let (library, group, paths) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);

    let form = cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_group_form(GroupIntent::Create(Some(group)), window, cx);
            app.project_pane.group_form.clone().unwrap()
        })
    });
    settle(cx);
    form.read_with(cx, |form, _| {
        assert_eq!(form.parent.as_deref(), Some("Work"))
    });
    cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            form.input
                .update(cx, |input, cx| input.set_value("Clients", window, cx));
            form.submit(window, cx);
        })
    });
    settle_save(&app, cx).await;

    let nested = app.read_with(cx, |app, _| {
        let entries = app.project_library.groups();
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.name.as_str(), entry.depth))
                .collect::<Vec<_>>(),
            vec![("Work", 0), ("Clients", 1)]
        );
        assert!(app.project_pane.group_form.is_none());
        entries[1].id
    });
    assert_eq!(Preferences::load().project_library.groups().len(), 2);

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.edit_project_library(
                |library| {
                    library.remove_group(nested);
                    library.remove_group(group);
                    Ok(())
                },
                window,
                cx,
            )
        })
    });
    settle_save(&app, cx).await;
    let saved = Preferences::load();
    assert!(saved.project_library.groups().is_empty());
    for path in &paths {
        assert!(saved.project_library.contains_project(path));
    }
}

#[gpui::test]
async fn a_group_dialog_shows_its_own_failed_save_beside_a_newer_one(cx: &mut TestAppContext) {
    let (library, group, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    // A directory where the store should be makes the group's save fail.
    let store = crate::preferences::settings_path().unwrap();
    fs::create_dir_all(&store).unwrap();

    let form = cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_group_form(GroupIntent::Create(None), window, cx);
            app.project_pane.group_form.clone().unwrap()
        })
    });
    settle(cx);
    let repair = cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            form.input
                .update(cx, |input, cx| input.set_value("Clients", window, cx));
            form.submit(window, cx);
        });
        // The writer runs in submission order: the group's save fails, the
        // store is repaired, and a newer pane save succeeds, so the pane
        // reports no failure when the group's reply is handled.
        app.update(cx, |app, cx| {
            let repaired = store.clone();
            let repair = app
                .preferences_writer
                .submit(move || Ok(fs::remove_dir(&repaired)?));
            app.edit_project_library(
                |library| {
                    library.set_collapsed(group, true);
                    Ok(())
                },
                window,
                cx,
            );
            repair
        })
    });
    repair.await.unwrap().unwrap();
    settle_save(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(app.project_pane.error.is_none());
        assert!(app.project_pane.group_form.is_some());
    });
    form.read_with(cx, |form, _| {
        assert!(!form.pending);
        assert!(
            form.error
                .as_deref()
                .is_some_and(|error| error.starts_with("Could not save the project list"))
        );
    });
}

/// A write refuses a project's row at once: assistive technology hears it
/// unavailable from the write's start, and a click leaves the cursor where it
/// was and opens nothing, while the row dims only once the write looks busy
/// (`workspace::BusyLook`). The click once moved the cursor onto the row
/// before the open was refused, and the row was never announced unavailable.
#[gpui::test]
async fn a_write_refuses_a_project_row_before_it_moves_anything(cx: &mut TestAppContext) {
    use crate::workspace::refusals::{self, Refusal};
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    let index = app.read_with(cx, |app, _| {
        app.project_pane
            .rows
            .iter()
            .position(|row| matches!(row, PaneRow::Project { .. }))
            .expect("a project row")
    });
    let key = format!("project-pane-row-{index}");
    let row = |cx: &mut VisualTestContext| {
        refusals::forget();
        settle(cx);
        refusals::get(&key)
    };
    let state = |announced, dimmed| Some(Refusal { announced, dimmed });
    assert_eq!(row(cx), state(false, false), "at rest");
    app.update(cx, |app, cx| {
        app.operation_busy = Some("Staging files…");
        cx.notify();
    });
    assert_eq!(row(cx), state(true, false), "as the write starts");
    let cursor = app.read_with(cx, |app, _| app.project_pane.cursor);
    assert_ne!(cursor, Some(index));
    let bounds = cx
        .debug_bounds(format!("project-pane-row-{index}").leak())
        .expect("the project's row");
    cx.simulate_click(bounds.center(), Modifiers::default());
    settle(cx);
    // The click focuses the tree, as a click in any list does; the row's own
    // activation, which moved the cursor onto it first, is refused.
    assert_eq!(
        app.read_with(cx, |app, _| app.project_pane.cursor),
        cursor,
        "the refused click leaves the cursor where it was"
    );
    app.read_with(cx, |app, _| assert_eq!(app.page, AppPage::Settings));
    cx.executor()
        .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
    cx.run_until_parked();
    assert_eq!(row(cx), state(true, true), "once the write looks busy");
    app.update(cx, |app, cx| {
        app.operation_busy = None;
        cx.notify();
    });
    assert_eq!(row(cx), state(false, false), "once the write ends");
}

/// A project added above the tree's cursor row leaves the cursor on its
/// project, and with it the menus open on its row: Shift-F10's stays open,
/// drawn below the project's row where it now stands, and the actions
/// button's stays open beside the project's row as it moves. Keyed by their
/// place, Shift-F10's menu closed and the button's stayed beside the place,
/// on the project that moved into it.
#[gpui::test]
fn a_project_rows_menus_stay_with_it_as_a_row_arrives_above(cx: &mut TestAppContext) {
    const NAME: &str = "menu-item: beta · Label";
    let (library, _, paths) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|_, cx| native_accessibility::bind_keys(cx));
    let beta = RowKey::Project(paths[1].as_path().into());
    let row_of = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| {
            app.project_pane
                .rows
                .iter()
                .position(|row| row.key() == beta)
                .expect("beta in the tree")
        })
    };
    let arrive = |cx: &mut VisualTestContext, name: &str| {
        app.update(cx, |app, cx| {
            let mut library = app.project_library.clone();
            assert!(library.remember(&Path::new("/projects").join(name)));
            app.set_project_library(library);
            cx.notify();
        });
        settle(cx);
    };
    let focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| app.read(cx).project_pane.focus.is_focused(window))
    };

    // Shift-F10's menu, on the cursor row.
    let at = row_of(cx);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.project_pane.cursor = Some(at);
            window.focus(&app.project_pane.focus, cx);
        })
    });
    settle(cx);
    cx.simulate_keystrokes("shift-f10");
    settle(cx);
    assert!(
        cx.debug_bounds(NAME).is_some(),
        "Shift-F10 opened beta's menu"
    );
    arrive(cx, "aaa");
    let moved = row_of(cx);
    assert!(moved > at, "a row arrived above beta's");
    assert_eq!(
        app.read_with(cx, |app, _| app.project_pane.cursor),
        Some(moved),
        "the cursor stays on beta"
    );
    assert!(
        app.read_with(cx, |app, _| app.project_pane.keyboard_menu.is_some()),
        "the menu stays open on beta"
    );
    let row = cx
        .debug_bounds(format!("project-pane-row-{moved}").leak())
        .expect("beta's row");
    let name = cx.debug_bounds(NAME).expect("the menu names beta");
    assert!(
        name.top() >= row.bottom(),
        "the menu {name:?} lies below beta's row {row:?} where it now stands"
    );
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(focused(cx), "Escape returns focus to the tree");

    // The actions button's menu, on beta's row.
    let before = cx
        .debug_bounds(format!("project-pane-row-{moved}").leak())
        .expect("beta's row");
    let button = cx
        .debug_bounds(format!("project-row-menu-{moved}").leak())
        .expect("beta's actions button");
    cx.simulate_mouse_move(button.center(), None, Modifiers::default());
    settle(cx);
    cx.simulate_click(button.center(), Modifiers::default());
    settle(cx);
    let opened = cx
        .debug_bounds(NAME)
        .expect("the button opened beta's menu");
    arrive(cx, "aab");
    let now = row_of(cx);
    assert!(now > moved, "another row arrived above beta's");
    let after = cx
        .debug_bounds(format!("project-pane-row-{now}").leak())
        .expect("beta's row");
    let name = cx.debug_bounds(NAME).expect("the button's menu stays open");
    let (row_moved, menu_moved) = (after.top() - before.top(), name.top() - opened.top());
    assert!(
        (row_moved - menu_moved).abs() < px(0.5),
        "the menu moved {menu_moved:?} with beta's row, which moved {row_moved:?}"
    );
    assert_eq!(
        app.read_with(cx, |app, _| app.project_pane.menu_open.clone()),
        Some(beta.clone()),
        "beta's menu is the one open"
    );
    cx.simulate_keystrokes("escape");
    settle(cx);
}

/// A project row's key and element ID share the row's path rather than
/// copying it, so drawing the row allocates nothing for either: the ID
/// once copied the path into a new allocation in every frame.
#[test]
fn a_project_rows_key_and_id_share_its_path() {
    let (library, _, _) = library_with_group();
    let rows = present_rows(&library, folder_name);
    let (row, path) = rows
        .iter()
        .find_map(|row| match row {
            PaneRow::Project { path, .. } => Some((row, path)),
            PaneRow::Group { .. } => None,
        })
        .expect("a project row");
    let key = row.key();
    assert!(
        matches!(&key, RowKey::Project(shared) if std::sync::Arc::ptr_eq(shared, path)),
        "the key shares the row's path"
    );
    assert!(
        matches!(key.element_id(), ElementId::Path(shared) if std::sync::Arc::ptr_eq(&shared, path)),
        "the element ID shares it too"
    );
}

/// A row's menu opened from the keyboard lies below its row with the gap
/// every menu keeps from what it opened from, 4 px at 13 pt and scaled with
/// the text, between the row's foot and the menu's drawn edge, the ring
/// outside its fill (`appearance::popup_gap`): its fill started at the
/// row's foot, so its edge lay on the row.
#[gpui::test]
fn a_keyboard_row_menu_keeps_the_menu_gap_below_its_row(cx: &mut TestAppContext) {
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    let edge = gpui_kit::component::POPOVER_EDGE_WIDTH;
    for text in [13, 18] {
        cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
        settle(cx);
        // The first row, the group, at the list's top.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.move_project_cursor(false, true, window, cx)
            })
        });
        settle(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.manage_project_row(window, cx)));
        settle(cx);
        let row = cx
            .debug_bounds("project-pane-row-0")
            .expect("the group's row");
        let label = cx
            .debug_bounds("menu-item: Work · Label")
            .expect("the row's menu");
        let surface = cx.update(|window, cx| {
            let fill = gpui_kit::Background::from(gpui_kit::component::Theme::global(cx).popover);
            let scale = window.scale_factor();
            let logical = |value: gpui_kit::ScaledPixels| px(value.as_f32() / scale);
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| quad.background == fill)
                .map(|quad| {
                    let bounds = Bounds::from_corners(
                        point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                        point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                    );
                    (quad.order, bounds)
                })
                .filter(|(_, surface)| surface.contains(&label.center()))
                .max_by_key(|(order, _)| *order)
                .map(|(_, surface)| surface)
                .expect("the menu's surface")
        });
        let gap = surface.top() - edge - row.bottom();
        let expected = crate::appearance::popup_gap();
        assert!(
            (gap - expected).abs() <= px(0.5),
            "{text} pt: the menu {surface:?} draws its edge {gap:?} below the row {row:?}, not {expected:?}"
        );
        cx.simulate_keystrokes("escape");
        settle(cx);
    }
}

/// A row's menu closes as the pane leaves, held back in a window too narrow
/// to draw it beside the page, and neither comes back as the window widens:
/// the keyboard's menu, opened by Shift+F10, and the actions button's. The
/// keyboard's menu was drawn again beside its row as the window widened,
/// open with focus elsewhere, and the actions button's stayed recorded open,
/// its row keeping no tooltip.
#[gpui::test]
fn a_rows_menu_closes_as_the_window_holds_the_pane_back(cx: &mut TestAppContext) {
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, enabled(), library);
    cx.update(|_, cx| native_accessibility::bind_keys(cx));
    let item = "menu-item: Open project · MenuItem";
    let open = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| {
            (
                app.project_pane.keyboard_menu.is_some(),
                app.project_pane.menu_open.is_some(),
            )
        })
    };
    // The last row, beta, a project.
    let beta = app.read_with(cx, |app, _| app.project_pane.rows.len() - 1);
    for menu in ["Shift+F10", "the actions button"] {
        cx.simulate_resize(size(px(1200.), px(720.)));
        settle(cx);
        if menu == "Shift+F10" {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.move_project_cursor(true, true, window, cx);
                    window.focus(&app.project_pane.focus, cx);
                })
            });
            settle(cx);
            cx.simulate_keystrokes("shift-f10");
        } else {
            let button = cx
                .debug_bounds(format!("project-row-menu-{beta}").leak())
                .expect("beta's actions button");
            cx.simulate_mouse_move(button.center(), None, Modifiers::default());
            settle(cx);
            cx.simulate_click(button.center(), Modifiers::default());
        }
        settle(cx);
        assert!(cx.debug_bounds(item).is_some(), "{menu} opened beta's menu");
        assert_ne!(open(cx), (false, false), "{menu}: recorded open");
        cx.simulate_resize(size(px(461.), px(490.)));
        settle(cx);
        assert!(
            cx.debug_bounds("project-pane").is_none(),
            "{menu}: held back"
        );
        assert!(cx.debug_bounds(item).is_none(), "{menu}: the menu went");
        assert_eq!(open(cx), (false, false), "{menu}: closed with the pane");
        let (focused, tree, root) = cx.update(|window, cx| {
            let app = app.read(cx);
            (
                window.focused(cx),
                app.project_pane.focus.clone(),
                app.app_focus.clone(),
            )
        });
        let focused = focused.unwrap_or_else(|| panic!("{menu}: focus stays on a control"));
        assert!(
            focused != tree && focused != root,
            "{menu}: focus is on one of the page's controls"
        );
        assert!(
            cx.update(|window, _| root.contains(&focused, window)),
            "{menu}: on a control the frame draws"
        );
        cx.simulate_resize(size(px(1200.), px(720.)));
        settle(cx);
        assert!(cx.debug_bounds("project-pane").is_some(), "{menu}: drawn");
        assert!(cx.debug_bounds(item).is_none(), "{menu}: not open again");
        assert_eq!(open(cx), (false, false), "{menu}: still closed");
    }
}

/// The pane's row's fresh capture shows and acts on nothing from the Git
/// directory the tab had until its own status read lands, as the Projects
/// page's row's does
/// (`repository_tabs::tests::assert_a_fresh_capture_shows_nothing_from_before`).
#[gpui::test]
async fn a_pane_row_s_fresh_capture_shows_nothing_from_before(cx: &mut TestAppContext) {
    crate::repository_tabs::tests::assert_a_fresh_capture_shows_nothing_from_before(
        cx,
        |app, cx, root| {
            app.update(cx, |app, cx| {
                let mut library = app.project_library.clone();
                library.remember(root);
                app.set_project_library(library);
                app.settings.project_pane = true;
                cx.notify();
            });
            crate::views::tests::draw(cx);
            let index = app.read_with(cx, |app, _| {
                (app.project_pane.rows.iter())
                    .position(|row| matches!(row, PaneRow::Project { path, .. } if **path == *root))
                    .expect("the project's row")
            });
            let row = cx
                .debug_bounds(format!("project-pane-row-{index}").leak())
                .expect("the pane's row drawn");
            cx.simulate_click(row.center(), Modifiers::default());
        },
    )
    .await;
}

/// The pane's row for a marked tab is an explicit Open of its folder too: the
/// tab takes the repository found there, its mark gone. Selecting the shown
/// project returned early, as it should for a tab that is whole, so the tab
/// stayed unavailable.
#[gpui::test]
async fn a_pane_row_captures_a_marked_tab_afresh(cx: &mut TestAppContext) {
    let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
    let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let third = crate::views::tests::fixture_repository(fixture.path().join("third"));
    let third_git = third.git_dir().to_owned();
    crate::views::tests::hold_quiet_reads_for_test(&app, cx);
    fs::rename(root.join(".git"), fixture.path().join("git-away")).unwrap();
    fs::write(
        root.join(".git"),
        format!("gitdir: {}\n", third_git.display()),
    )
    .unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&crate::Refresh, window, cx)));
    crate::views::tests::settle(&app, cx).await;
    assert!(
        app.read_with(cx, |app, _| app.shown_tab_unavailable()),
        "Refresh marks the tab"
    );

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_project_from_pane(root.clone(), window, cx)
        })
    });
    crate::views::tests::settle(&app, cx).await;
    app.read_with(cx, |app, _| {
        assert!(!app.shown_tab_unavailable(), "its mark cleared");
        assert_eq!(app.error, None);
        assert_eq!(
            app.repository.as_ref().map(|repo| repo.git_dir()),
            Some(third_git.as_path()),
            "the repository found there"
        );
    });
}

/// One Enter key event as the platform delivers it, with no idle pass after
/// it: a key-down, marked held or not, or with `None` its key-up.
fn enter(cx: &mut VisualTestContext, down: Option<bool>) {
    let keystroke = Keystroke::parse("enter").expect("a keystroke");
    let input = match down {
        Some(is_held) => PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held,
            prefer_character_input: false,
        }),
        None => PlatformInput::KeyUp(KeyUpEvent { keystroke }),
    };
    cx.update(|window, cx| window.dispatch_event(input, cx));
}

/// A held Return in the group name field begins one save, under X11 too,
/// and an empty name's refusal is given once per press: with the store
/// refusing writes, Enter goes down and the save fails, then the key's
/// repeats arrive as X11 sends them, fresh presses with no key-up between
/// them and a release and its press together, and as GPUI marks them held
/// on macOS and Wayland, each waited out until the writer has answered. The
/// form saves once and says why once, on one node announced once; after the
/// key-up a new press saves again and is heard again. Emptied, the name is
/// refused once per press the same way. The field's own `PressEnter`
/// submitted the form as well as the dialog's default action, so each
/// repeat began another write after the last failed, and every press gave
/// an empty name's refusal twice.
#[gpui::test]
async fn a_held_return_in_a_group_name_submits_once_per_press(cx: &mut TestAppContext) {
    use crate::native_accessibility::written;
    let (app, cx) = test_app(cx, enabled(), ProjectLibrary::default());
    // A directory where the store should be refuses the save, as in
    // `a_group_dialog_shows_its_own_failed_save_beside_a_newer_one`.
    let store = crate::preferences::settings_path().unwrap();
    fs::create_dir_all(&store).unwrap();
    let form = cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_group_form(GroupIntent::Create(None), window, cx);
            app.project_pane.group_form.clone().unwrap()
        })
    });
    settle(cx);
    cx.simulate_input("Clients");
    // Saves begun, counted as the form changes, and refusals given.
    let saves = Rc::new(std::cell::Cell::new(0_usize));
    let _counting = cx.update(|_, cx| {
        let saves = saves.clone();
        let mut was = false;
        cx.observe(&form, move |form, cx| {
            let now = form.read(cx).pending;
            saves.set(saves.get() + usize::from(now && !was));
            was = now;
        })
    });
    let seen =
        |cx: &mut VisualTestContext| (saves.get(), form.read_with(cx, |form, _| form.errors_shown));
    let heard = || written::announced_from("project-group-error").len();
    for (case, step) in [("a refused save", (1, 1)), ("an empty name", (0, 1))] {
        if case == "an empty name" {
            cx.update(|window, cx| {
                let input = form.read(cx).input.clone();
                input.update(cx, |input, cx| input.set_value("", window, cx));
            });
            settle(cx);
        }
        let before = seen(cx);
        let after = (before.0 + step.0, before.1 + step.1);
        written::forget();
        enter(cx, Some(false));
        settle_save(&app, cx).await;
        assert_eq!(seen(cx), after, "{case}: the press is answered once");
        assert_eq!(heard(), 1, "{case}: the refusal is heard once");
        let repeats: [(&str, &[Option<bool>]); 4] = [
            ("an X11 repeat", &[Some(false)]),
            ("another X11 repeat", &[Some(false)]),
            // A release and its press together, with no idle pass between.
            ("a release and its press", &[None, Some(false)]),
            ("a held repeat", &[Some(true)]),
        ];
        for (repeat, events) in repeats {
            for down in events {
                enter(cx, *down);
            }
            settle_save(&app, cx).await;
            cx.update(|window, cx| {
                assert!(
                    window.has_active_dialog(cx),
                    "{case}, {repeat}: the dialog stays"
                );
                let form = form.read(cx);
                assert!(
                    form.visible && !form.pending,
                    "{case}, {repeat}: the form waits"
                );
                assert!(
                    form.input.read(cx).focus_handle(cx).is_focused(window),
                    "{case}, {repeat}: focus stays in the field"
                );
            });
            assert_eq!(
                seen(cx),
                after,
                "{case}, {repeat}: nothing more is begun or said"
            );
            assert_eq!(heard(), 1, "{case}, {repeat}: no other refusal is heard");
        }
        enter(cx, None);
        settle(cx);
        written::forget();
        enter(cx, Some(false));
        settle_save(&app, cx).await;
        assert_eq!(
            seen(cx),
            (after.0 + step.0, after.1 + step.1),
            "{case}: a new press after the key-up is answered again"
        );
        assert_eq!(heard(), 1, "{case}: and heard again");
        enter(cx, None);
        settle(cx);
    }
}
