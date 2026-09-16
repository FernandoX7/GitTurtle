use super::*;
use crate::{
    AppPage, activity, image_lifetime, preferences::AppSettings, recovery_drafts, repository_tabs,
};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use std::{cell::RefCell, path::Path, rc::Rc};

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

#[gpui::test]
fn the_pane_appears_only_when_it_is_enabled_and_a_page_needs_it(cx: &mut TestAppContext) {
    let (library, _, _) = library_with_group();
    let (app, cx) = test_app(cx, AppSettings::default(), library.clone());
    app.read_with(cx, |app, _| assert_eq!(app.page, AppPage::Settings));
    assert!(cx.debug_bounds("project-pane").is_none());

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings.project_pane = true;
            app.save_preferences(window, cx);
        })
    });
    settle(cx);
    let pane = cx.debug_bounds("project-pane").expect("enabled pane");
    assert!(pane.size.width > px(0.));
    // The pane sits at the far left, under the tab strip and header.
    assert!(pane.origin.x < px(1.));

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
