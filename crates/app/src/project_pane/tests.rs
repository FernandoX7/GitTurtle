use super::*;
use crate::{
    AppPage, activity, image_lifetime, preferences::AppSettings, recovery_drafts, repository_tabs,
};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::{cell::RefCell, fs, path::Path, rc::Rc};

fn test_app(
    cx: &mut TestAppContext,
    settings: AppSettings,
    library: ProjectLibrary,
) -> (Entity<GitTurtle>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        image_lifetime::init(cx);
    });
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
            &RowKey::Project(project.to_owned())
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
            PaneRow::Project { path, depth: 1, .. } if *path == paths[0]
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
            PaneRow::Project { path, depth: 0, .. } if *path == paths[0]
        ));
    });
}

#[gpui::test]
fn an_older_save_reply_never_replaces_a_newer_edit(cx: &mut TestAppContext) {
    let (library, group, paths) = library_with_group();
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
            app.set_project_library(newer.clone());
            app.project_pane.pending_saves = 2;
            // The first reply is for a write that preceded the rename.
            app.finish_project_library_save(&Ok(older), cx);
            assert_eq!(app.project_library, newer);
            assert_eq!(app.project_pane.pending_saves(), 1);
            // A recent-project save answering now is older too.
            app.absorb_saved_project_library(library.clone());
            assert_eq!(app.project_library, newer);
            app.finish_project_library_save(&Ok(newest), cx);
            assert_eq!(app.project_library, newer);
            assert_eq!(app.project_pane.pending_saves(), 0);
            app.absorb_saved_project_library(library.clone());
            assert_eq!(app.project_library, library);
            assert!(app.project_library.contains_project(&paths[1]));
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
    app.read_with(cx, |app, _| {
        assert!(
            app.project_pane
                .error
                .as_deref()
                .is_some_and(|error| error.starts_with("Could not save the project list"))
        );
        // The edit stays on screen so Retry can write it.
        assert!(app.project_library.group(group).unwrap().collapsed);
        assert!(app.operation_error.is_none());
    });

    fs::remove_dir(&store).unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.retry_project_library_save(window, cx)));
    settle_save(&app, cx).await;
    assert!(cx.debug_bounds("project-pane-error").is_none());
    app.read_with(cx, |app, _| assert!(app.project_pane.error.is_none()));
    assert!(
        Preferences::load()
            .project_library
            .group(group)
            .unwrap()
            .collapsed
    );
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
