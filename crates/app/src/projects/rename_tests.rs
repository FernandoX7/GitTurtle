use super::*;
use crate::{activity, image_lifetime, preferences, recovery_drafts, repository_tabs};
use gpui_kit::component::Root;

fn test_app(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        image_lifetime::init(cx);
    });
    let fixture = tempfile::tempdir().unwrap();
    let save_path = fixture.path().join("isolated-session.json");
    let captured = Rc::new(RefCell::new(None));
    let observed = captured.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| {
            let mut app = GitTurtle::new(
                None,
                Preferences::default(),
                repository_tabs::Session::default(),
                activity::State::default(),
                recovery_drafts::State::default(),
                window,
                cx,
            );
            app.repository_tabs.save_path = Some(save_path);
            app
        });
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = observed.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1000.), px(680.)));
    (fixture, app, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
}

async fn settle_save(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let barrier = app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())));
    barrier.await.unwrap().unwrap();
    settle(cx);
}

fn open_form(
    app: &Entity<GitTurtle>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<RenameProjectForm> {
    let form = cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_rename_project(path.to_owned(), window, cx);
            app.rename_project.clone().unwrap()
        })
    });
    settle(cx);
    form
}

#[gpui::test]
fn rename_focus_selects_saved_name_and_cancel_reopens_saved_value(cx: &mut TestAppContext) {
    let (fixture, app, cx) = test_app(cx);
    let path = fixture.path().join("client-folder");
    let saved = "Client café";
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            app.project_names.insert(path.clone(), saved.into());
        });
    });
    let first = open_form(&app, &path, cx);
    cx.update(|window, cx| {
        let input = first.read(cx).input.read(cx);
        assert!(input.focus_handle(cx).is_focused(window));
        assert_eq!(input.selected_range(), 0..saved.len());
    });
    cx.simulate_input("Unsaved replacement");
    cx.update(|window, cx| {
        assert_eq!(first.read(cx).input.read(cx).value(), "Unsaved replacement");
        window.dispatch_action(Box::new(Cancel), cx);
    });
    settle(cx);
    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert!(!first.read(cx).visible);
        assert_eq!(app.read(cx).project_name(&path), saved);
    });

    let reopened = open_form(&app, &path, cx);
    assert_ne!(first.entity_id(), reopened.entity_id());
    cx.update(|window, cx| {
        let input = reopened.read(cx).input.read(cx);
        assert_eq!(input.value(), saved);
        assert_eq!(input.selected_range(), 0..saved.len());
        assert!(input.focus_handle(cx).is_focused(window));
    });
}

#[gpui::test]
fn invalid_project_name_keeps_the_dialog_and_typed_text(cx: &mut TestAppContext) {
    let (fixture, app, cx) = test_app(cx);
    let path = fixture.path().join("client-folder");
    let form = open_form(&app, &path, cx);
    let invalid = "x".repeat(129);
    cx.simulate_input(&invalid);
    cx.simulate_keystrokes("enter");
    settle(cx);
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        let form = form.read(cx);
        assert!(form.visible);
        assert!(!form.pending);
        assert!(
            form.error
                .as_ref()
                .is_some_and(|error| error.contains("128"))
        );
        assert_eq!(form.input.read(cx).value().as_ref(), invalid);
        assert!(!app.read(cx).project_names.contains_key(&path));
    });
}

#[gpui::test]
async fn pending_rename_blocks_duplicate_submission_edits_and_cancel(cx: &mut TestAppContext) {
    let (fixture, app, cx) = test_app(cx);
    let path = fixture.path().join("client-folder");
    let form = open_form(&app, &path, cx);
    cx.simulate_input("Reviewed name");
    let (release, gate) = std::sync::mpsc::channel();
    let (started, running) = futures::channel::oneshot::channel();
    let blocker = app.read_with(cx, |app, _| {
        app.preferences_writer.submit(move || {
            let _ = started.send(());
            gate.recv()?;
            Ok(())
        })
    });
    running.await.unwrap();
    cx.simulate_keystrokes("enter");
    settle(cx);
    cx.simulate_input("must not change the accepted name");
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
        window.dispatch_action(Box::new(Cancel), cx);
        form.update(cx, |form, cx| form.submit(window, cx));
    });
    cx.simulate_keystrokes("escape");
    settle(cx);
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        let form = form.read(cx);
        assert!(form.visible && form.pending);
        assert!(form.error.is_none());
        assert_eq!(form.input.read(cx).value(), "Reviewed name");
        assert!(!app.read(cx).project_names.contains_key(&path));
    });
    release.send(()).unwrap();
    blocker.await.unwrap().unwrap();
    settle_save(&app, cx).await;
    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert_eq!(app.read(cx).project_name(&path), "Reviewed name");
    });
}

#[gpui::test]
async fn failed_rename_retains_text_until_success_updates_hub_and_closes(cx: &mut TestAppContext) {
    let (fixture, app, cx) = test_app(cx);
    // Test preferences are owned by this test thread and inherited by its
    // SerialExecutor, independently of the user's application data.
    let settings_path = preferences::settings_path().unwrap();
    let invalid_store = b"{ fixture invalid preferences";
    std::fs::write(&settings_path, invalid_store).unwrap();
    let path = fixture.path().join("client-folder");
    let form = open_form(&app, &path, cx);
    cx.simulate_input("Chosen name");
    cx.simulate_keystrokes("enter");
    settle_save(&app, cx).await;
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        let form = form.read(cx);
        assert!(form.visible && !form.pending);
        assert_eq!(form.input.read(cx).value(), "Chosen name");
        assert!(
            form.error
                .as_ref()
                .unwrap()
                .contains("Could not save the project name")
        );
        assert!(!app.read(cx).project_names.contains_key(&path));
    });
    assert_eq!(std::fs::read(&settings_path).unwrap(), invalid_store);

    std::fs::remove_file(&settings_path).unwrap();
    cx.simulate_keystrokes("enter");
    settle_save(&app, cx).await;
    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert!(!form.read(cx).visible);
        assert!(!form.read(cx).pending);
        let app = app.read(cx);
        assert!(app.rename_project.is_none());
        assert_eq!(app.project_name(&path), "Chosen name");
        assert_eq!(app.hub.read(cx).display_name(&path), "Chosen name");
    });
    assert_eq!(
        Preferences::load()
            .project_names
            .get(&path)
            .map(String::as_str),
        Some("Chosen name")
    );
}

#[gpui::test]
fn late_rename_completion_does_not_close_a_different_project_dialog(cx: &mut TestAppContext) {
    let (fixture, app, cx) = test_app(cx);
    let first_path = fixture.path().join("first-client");
    let second_path = fixture.path().join("second-client");
    let first = open_form(&app, &first_path, cx);
    cx.update(|window, cx| {
        first.update(cx, |form, _| {
            form.pending = true;
            form.visible = false;
        });
        // Model the old view having been dismissed/replaced independently of
        // its accepted background write.
        window.close_dialog(cx);
    });
    let second = open_form(&app, &second_path, cx);
    cx.simulate_input("Second draft");
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.finish_project_rename(
                first.downgrade(),
                Ok(Preferences {
                    project_names: HashMap::from([(first_path.clone(), "First saved".into())]),
                    ..Default::default()
                }),
                window,
                cx,
            );
        });
        assert!(window.has_active_dialog(cx));
        assert!(second.read(cx).visible);
        assert_eq!(second.read(cx).input.read(cx).value(), "Second draft");
        let app = app.read(cx);
        assert_eq!(
            app.rename_project.as_ref().unwrap().entity_id(),
            second.entity_id()
        );
        assert_eq!(app.project_name(&first_path), "First saved");
        assert_eq!(app.project_name(&second_path), "second-client");
    });
}
