//! The header profile menu's rows as the kit announces them: the AccessKit
//! node each entry writes, which the kit names in its debug selector
//! (`menu-item: <label> · <role>`), and each label's drawn style.
use super::*;
use crate::native_accessibility::menu_tests::menu_label_style;
use crate::views::tests::{history_window, settle};
use core::prelude::v1::test;
use gitturtle_core::GitProfile;

fn definition(id: &str, label: &str, email: &str) -> Definition {
    Definition {
        id: id.into(),
        label: label.into(),
        name: "GitTurtle QA".into(),
        email: email.into(),
        signing: None,
    }
}

fn shown(cx: &mut VisualTestContext, entry: &str) -> bool {
    cx.debug_bounds(entry.to_owned().leak()).is_some()
}

/// Settle the profile store's load, then give the app `store`, so a late
/// load cannot replace it.
async fn use_store(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, store: Store) {
    let barrier = app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())));
    barrier.await.unwrap().unwrap();
    settle(app, cx).await;
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            app.profiles.store = Some(store);
            app.profiles.error = None;
        })
    });
    settle(app, cx).await;
}

/// Open the menu, which is built as it opens, with `profile` as the
/// identity Git authors with. It is set just before the click, so a quiet
/// refresh of the fixture's own identity cannot replace it first.
async fn open_menu(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let picker = cx
        .debug_bounds("profile-button")
        .expect("the profile picker");
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            app.profile = Some(GitProfile {
                name: "GitTurtle QA".into(),
                email: "qa@example.invalid".into(),
                ..Default::default()
            })
        })
    });
    cx.simulate_click(picker.center(), Modifiers::default());
    settle(app, cx).await;
}

/// The profile menu says what each row is. The identity Git authors with now
/// leads with "Current:"; the saved profiles sit under the heading "Saved Git
/// profiles", a label drawn as a section label and not a command, or, with
/// none saved, one line says so instead of titling an empty group; and New
/// profile… is an action that opens the profile form, beside Manage
/// profiles…. The former heading's command ellipsis and the hint that read
/// as an action are gone.
#[gpui::test]
async fn the_profile_menu_names_its_rows(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    let mut store = Store::default();
    store.definitions = vec![
        definition("work", "Work", "qa@work.example"),
        definition("personal", "Personal", "qa@home.example"),
    ];
    use_store(&app, cx, store).await;
    open_menu(&app, cx).await;

    let current = "Current: GitTurtle QA <qa@example.invalid>";
    let style = |label: &str| {
        menu_label_style(
            label,
            appearance::ui_text(appearance::SECTION_LABEL_TEXT),
            FontWeight::MEDIUM,
        )
    };
    for entry in [
        format!("menu-item: {current} · Label"),
        style(current),
        "menu-item: Saved Git profiles · Label".into(),
        style("Saved Git profiles"),
        "menu-item: Work · qa@work.example · MenuItemCheckBox False".into(),
        "menu-item: Personal · qa@home.example · MenuItemCheckBox False".into(),
        "menu-item: New profile… · MenuItem".into(),
        "menu-item: Manage profiles… · MenuItem".into(),
    ] {
        assert!(shown(cx, &entry), "{entry}");
    }
    for gone in [
        "Apply a saved Git profile…",
        "GitTurtle QA <qa@example.invalid>",
        "Create Personal, Work, or another identity",
        "No saved profiles",
    ] {
        for role in ["Label", "MenuItem", "MenuItem disabled"] {
            let entry = format!("menu-item: {gone} · {role}");
            assert!(!shown(cx, &entry), "not {entry}");
        }
    }

    cx.simulate_keystrokes("escape");
    use_store(&app, cx, Store::default()).await;
    open_menu(&app, cx).await;
    for entry in [
        "menu-item: No saved profiles · Label".into(),
        style("No saved profiles"),
        "menu-item: New profile… · MenuItem".into(),
    ] {
        assert!(shown(cx, &entry), "{entry}");
    }
    assert!(!shown(cx, "menu-item: Saved Git profiles · Label"));

    let create = cx
        .debug_bounds("menu-item: New profile… · MenuItem")
        .expect("New profile…");
    cx.simulate_click(create.center(), Modifiers::default());
    settle(&app, cx).await;
    let creating = app.read_with(cx, |app, cx| {
        app.profiles.draft.as_ref().is_some_and(|form| {
            let form = form.read(cx);
            form.visible && form.previous.is_none()
        })
    });
    assert!(
        creating,
        "New profile… opens the profile form for a new profile"
    );
}

/// The identity line leaves out what Git has not set rather than drawing
/// empty brackets.
#[test]
fn the_current_identity_leaves_out_what_is_not_set() {
    let identity = |name: &str, email: &str| {
        current_identity(&GitProfile {
            name: name.into(),
            email: email.into(),
            ..Default::default()
        })
    };
    assert_eq!(
        identity("GitTurtle QA", "qa@example.invalid"),
        "Current: GitTurtle QA <qa@example.invalid>"
    );
    assert_eq!(identity("GitTurtle QA", ""), "Current: GitTurtle QA");
    assert_eq!(
        identity(" ", "qa@example.invalid"),
        "Current: <qa@example.invalid>"
    );
    assert_eq!(identity("", ""), "Current: no Git identity");
}
