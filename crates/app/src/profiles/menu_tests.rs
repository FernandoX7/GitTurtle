//! The header profile menu's rows as the kit announces them: the AccessKit
//! node each entry writes, which the kit names in its debug selector
//! (`menu-item: <label> · <role>`), and each label's drawn style.
use super::*;
use crate::native_accessibility::menu_tests::menu_label_style;
use crate::views::tests::{
    draw, fixture_git, fixture_repository, history_window, hold_quiet_reads_for_test, settle,
};
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
pub(super) async fn use_store(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, store: Store) {
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

/// Open the menu, which is built as it opens, with "GitTurtle QA
/// <qa@example.invalid>" as the identity Git authors with.
async fn open_menu(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    open_menu_as(app, cx, "GitTurtle QA", "qa@example.invalid").await;
}

/// Open the menu with `name` and `email` as the identity Git authors with,
/// quiet reads held so none installs the fixture's own identity before the
/// menu is built: it is built at a draw after the click, and the click parks
/// between its press and its release.
async fn open_menu_as(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    name: &str,
    email: &str,
) {
    hold_quiet_reads_for_test(app, cx);
    let picker = cx
        .debug_bounds("profile-button")
        .expect("the profile picker");
    cx.update(|_, cx| {
        app.update(cx, |app, _| {
            app.profile = Some(GitProfile {
                name: name.into(),
                email: email.into(),
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

/// In a 461 × 490 window, narrower than a menu's 500 px maximum, the profile
/// menu keeps its drawn edge, the ring outside its fill, inside the window
/// less the popup's 8 px inset on either side, even with a long name and
/// email. The current identity and the saved profile that carries the email
/// keep one line each, as tall as a short row's, ending with an ellipsis
/// where the menu ends, and each is still announced by its whole text. The
/// menu used to run past the window's edge.
#[gpui::test]
async fn the_profile_menu_keeps_a_long_identity_inside_a_narrow_window(cx: &mut TestAppContext) {
    const NAME: &str = "Maximiliana Konstantinopoulou-Vanderbergh";
    const EMAIL: &str = "maximiliana.konstantinopoulou-vanderbergh@engineering.example.invalid";
    let (_fixture, app, cx) = history_window(cx).await;
    cx.simulate_resize(size(px(461.), px(490.)));
    let mut store = Store::default();
    store.definitions = vec![definition("work", "Work", EMAIL)];
    use_store(&app, cx, store).await;
    open_menu_as(&app, cx, NAME, EMAIL).await;

    let current = format!("Current: {NAME} <{EMAIL}>");
    let saved = format!("Work · {EMAIL}");
    let bounds = |cx: &mut VisualTestContext, selector: String| {
        cx.debug_bounds(selector.clone().leak())
            .unwrap_or_else(|| panic!("{selector}"))
    };
    // The menu's fill takes the whole 443 px, 9 px inside either side, its
    // drawn edge 8 px in: the long identity's row ends at the cap less the
    // menu's `p_1` (3.25 px at 13 pt).
    let (start, end) = (px(9.), px(461. - 9.));
    let identity = bounds(cx, format!("menu-item: {current} · Label"));
    assert!(
        (identity.right() - (end - px(3.25))).abs() < px(0.5),
        "{identity:?} ends at the cap"
    );
    // Each row inside the window's inset, named by its whole text.
    for row in [
        format!("menu-item: {current} · Label"),
        format!("menu-item: {saved} · MenuItemCheckBox False"),
        "menu-item: Manage profiles… · MenuItem".into(),
    ] {
        let shown = bounds(cx, row.clone());
        assert!(
            shown.left() >= start && shown.right() <= end,
            "{row}: {shown:?} outside {start:?}..{end:?}"
        );
    }
    // Each long text keeps one line inside its row, cut short of its whole
    // width, where a wrapped text would be two lines tall.
    let (muted, whole) = cx.update(|window, cx| {
        (
            gpui_kit::component::Theme::global(cx).muted_foreground,
            crate::views::shaped_width(
                &current,
                appearance::ui_text(appearance::SECTION_LABEL_TEXT),
                FontWeight::MEDIUM,
                window,
            ),
        )
    });
    let label = |text: &str| format!("menu-label: {text} · {:?}", gpui::Rgba::from(muted));
    let (long, short) = (
        bounds(cx, label(&current)),
        bounds(cx, label("Saved Git profiles")),
    );
    assert_eq!(
        long.size.height, short.size.height,
        "{current:?} on one line"
    );
    assert!(long.right() <= end && long.size.width < whole, "{long:?}");
    let (long, short) = (
        bounds(cx, format!("menu-row-text: {saved}")),
        bounds(cx, "menu-row-text: Manage profiles…".into()),
    );
    assert_eq!(long.size.height, short.size.height, "{saved:?} on one line");
    assert!(long.right() <= end, "{long:?}");
    cx.simulate_keystrokes("escape");
}

/// At 461 × 490, at 13 and 18 pt, a profile menu of twelve saved profiles
/// is taller than its room above the status bar. Its rows keep the height
/// and spacing the menu's rows have where they fit, with three profiles in
/// 1280 × 800, and it ends after its last whole row, scrolling the rest
/// with its scrollbar shown. A capped menu once squeezed its rows instead.
#[gpui::test]
async fn a_long_profile_menu_keeps_its_rows_and_scrolls(cx: &mut TestAppContext) {
    use crate::views::tests::{assert_menu_keeps_its_rows, menu_rows};
    let (_fixture, app, cx) = history_window(cx).await;
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    let saved: Vec<_> = (1..=12)
        .map(|n| {
            definition(
                &format!("p{n}"),
                &format!("Profile {n:02}"),
                &format!("p{n:02}@example.invalid"),
            )
        })
        .collect();
    let mut labels = vec![
        "Current: GitTurtle QA <qa@example.invalid>".to_string(),
        "Saved Git profiles".into(),
    ];
    labels.extend(
        saved
            .iter()
            .map(|it| format!("{} · {}", it.label, it.email)),
    );
    labels.extend(["New profile…".into(), "Manage profiles…".into()]);
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    for text in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        let at = format!("{text} pt in 1280 × 800");
        cx.simulate_resize(size(px(1280.), px(800.)));
        let mut store = Store::default();
        store.definitions = saved[..3].to_vec();
        use_store(&app, cx, store).await;
        draw(cx);
        open_menu(&app, cx).await;
        draw(cx);
        let uncapped = menu_rows(cx, &labels);
        assert_eq!(uncapped.len(), 7, "{at}: {uncapped:?}");
        let (_, folded) = assert_menu_keeps_its_rows(cx, &at, &uncapped, &uncapped);
        assert!(!folded, "{at}: every row fits");
        cx.simulate_keystrokes("escape");
        draw(cx);

        let at = format!("{text} pt in 461 × 490");
        cx.simulate_resize(size(px(461.), px(490.)));
        let mut store = Store::default();
        store.definitions = saved.clone();
        use_store(&app, cx, store).await;
        draw(cx);
        open_menu(&app, cx).await;
        draw(cx);
        let rows = menu_rows(cx, &labels);
        assert_eq!(rows.len(), labels.len(), "{at}: {rows:?}");
        let (surface, folded) = assert_menu_keeps_its_rows(cx, &at, &rows, &uncapped);
        assert!(folded, "{at}: rows lie past the menu's fold at {surface:?}");
        cx.simulate_keystrokes("escape");
        draw(cx);
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
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

/// A profile save that fails while its form shows is said once: by the
/// form's own line, through the window's alert, and not by the window's
/// error as well. Both said it, the form's line even while it was hidden.
#[gpui::test]
async fn a_failed_save_is_said_once_by_the_form_that_shows_it(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    use_store(&app, cx, Store::default()).await;
    // A folder where the store's file goes: the save cannot write it.
    std::fs::create_dir_all(Store::path().unwrap()).unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.edit_profile(None, window, cx)));
    settle(&app, cx).await;
    let form = app
        .read_with(cx, |app, _| app.profiles.draft.clone())
        .expect("the form opens");
    cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            for (field, value) in [
                (&form.label, "Work"),
                (&form.name, "Profile Author"),
                (&form.email, "work@example.invalid"),
            ] {
                field.update(cx, |field, cx| field.set_value(value, window, cx));
            }
            form.submit(window, cx);
        })
    });
    let barrier = app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())));
    barrier.await.unwrap().unwrap();
    settle(&app, cx).await;
    let error = form
        .read_with(cx, |form, _| form.error.clone())
        .expect("the save fails");
    assert_eq!(
        app.read_with(cx, |app, _| app.operation_error_text()),
        None,
        "the window's error says nothing more"
    );
    let (_, said) = crate::copy_feedback::tests::alerted(cx).expect("the form's line is said");
    assert_eq!(said, error);
}

/// Closes the profile menu and opens it again, built afresh.
async fn reopen(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("escape");
    settle(app, cx).await;
    open_menu(app, cx).await;
}

/// The commands the profile menu last built, once each is drawn under what
/// it reads as.
fn built(cx: &mut VisualTestContext) -> Vec<crate::branch_actions::PickerItem> {
    let items = PROFILE_MENU_ITEMS.with(|items| items.borrow().clone());
    assert!(!items.is_empty(), "the menu is built");
    for item in &items {
        drawn_name(cx, item);
    }
    items
}

/// What `item` reads as, once it is drawn under it: its command where it is
/// offered, and where it is refused its accessible name, which says why, its
/// reason after its command, in the name the menu keys rest on
/// (`PopupMenuItem::reason_stated`).
fn drawn_name(cx: &mut VisualTestContext, item: &crate::branch_actions::PickerItem) -> String {
    let name = if item.disabled {
        let name = item.name.clone().unwrap_or_else(|| item.label.clone());
        assert!(
            item.reason_stated && name.starts_with(&format!("{}, ", item.label)),
            "{item:?} says why it is refused"
        );
        name
    } else {
        item.label.clone()
    };
    let role = match item.checked {
        Some(ticked) => format!("MenuItemCheckBox {}", if ticked { "True" } else { "False" }),
        None => "MenuItem".into(),
    };
    let disabled = if item.disabled { " disabled" } else { "" };
    let drawn = format!("menu-item: {name} · {role}{disabled}");
    assert!(shown(cx, &drawn), "{drawn}");
    name
}

/// The accessible name of each command the profile menu last built refused,
/// once it is drawn under it and says why ([`drawn_name`]).
fn refused(cx: &mut VisualTestContext) -> Vec<String> {
    built(cx)
        .iter()
        .filter(|item| item.disabled)
        .map(|item| drawn_name(cx, item))
        .collect()
}

/// What each command the profile menu last built reads as, offered or
/// refused, once drawn ([`drawn_name`]).
fn rows(cx: &mut VisualTestContext) -> Vec<String> {
    built(cx).iter().map(|item| drawn_name(cx, item)).collect()
}

/// Every item the profile menu refuses says why, as the branch picker's do,
/// heard as well as seen, and the menu keys rest on it: while a write runs,
/// a profile is saved, the saved profiles are first read or the repository
/// is, the write, the save or the read it waits for, in the room its
/// command leaves; and where the saved profiles or the repository could not
/// be read, a reason of its own beside it, which no wait's end changes. The
/// open menu is built again as that reason changes, so a saved profile is
/// offered once the repository's read lands and every item once the save
/// ends. New profile… while the profiles were read or could not be, and
/// the saved profiles while the repository was, were dimmed with nothing to
/// say why, and stayed refused until the menu was opened again.
#[gpui::test]
async fn every_item_the_profile_menu_refuses_says_why(cx: &mut TestAppContext) {
    const WORK: &str = "Work · qa@work.example";
    const PERSONAL: &str = "Personal · qa@home.example";
    const CREATE: &str = "New profile…";
    const MANAGE: &str = "Manage profiles…";
    let (_fixture, app, cx) = history_window(cx).await;
    let store = || {
        let mut store = Store::default();
        store.definitions = vec![
            definition("work", "Work", "qa@work.example"),
            definition("personal", "Personal", "qa@home.example"),
        ];
        store
    };
    use_store(&app, cx, store()).await;
    let with = |names: &[&str], why: &str| -> Vec<String> {
        names.iter().map(|name| format!("{name}, {why}")).collect()
    };
    let set = |cx: &mut VisualTestContext, change: fn(&mut GitTurtle)| {
        app.update(cx, |app, cx| {
            change(app);
            cx.notify();
        });
        draw(cx);
    };

    open_menu(&app, cx).await;
    assert_eq!(
        refused(cx),
        Vec::<String>::new(),
        "at rest nothing is refused"
    );

    set(cx, |app| app.operation_busy = Some("Fetching remote…"));
    reopen(&app, cx).await;
    assert_eq!(
        refused(cx),
        with(
            &[WORK, PERSONAL, CREATE, MANAGE],
            "available when fetching remote finishes"
        ),
        "a write's items are named by the write"
    );
    set(cx, |app| app.operation_busy = None);

    // A save that starts with the menu open, as one after a profile's
    // application does, refuses every item that acts until it ends.
    reopen(&app, cx).await;
    set(cx, |app| app.profiles.saving = true);
    assert_eq!(
        refused(cx),
        with(
            &[WORK, PERSONAL, CREATE, MANAGE],
            "available when saving profile finishes"
        ),
        "a save refuses its items, built again as it starts"
    );
    set(cx, |app| app.profiles.saving = false);
    assert_eq!(
        refused(cx),
        Vec::<String>::new(),
        "the save's end offers them again"
    );

    set(cx, |app| app.profiles.store = None);
    reopen(&app, cx).await;
    assert_eq!(
        refused(cx),
        with(&[CREATE], "available when loading profiles finishes"),
        "New profile… waits for the saved profiles it joins"
    );
    set(cx, |app| {
        app.profiles.error = Some("store unreadable".into())
    });
    assert_eq!(
        refused(cx),
        with(&[CREATE], "storage unavailable"),
        "built again as the read fails, New profile… says why beside it"
    );
    let muted = cx.update(|_, cx| gpui_kit::component::Theme::global(cx).muted_foreground);
    let reason = format!(
        "menu-detail: {STORAGE_UNAVAILABLE} · {:?}",
        gpui::Rgba::from(muted)
    );
    assert!(
        shown(cx, &reason),
        "the reason is drawn beside it in secondary text"
    );
    assert!(
        shown(cx, "menu-item: Retry profile storage · MenuItem"),
        "the way to read them again is offered"
    );
    cx.simulate_keystrokes("escape");
    use_store(&app, cx, store()).await;

    // The repository is read, as it is when it opens, then read again
    // without success.
    let repository = app.update(cx, |app, _| app.repository.take());
    set(cx, |app| app.loading = Some("Reading local history…"));
    open_menu(&app, cx).await;
    assert_eq!(
        refused(cx),
        with(
            &[WORK, PERSONAL],
            "available when reading local history finishes"
        ),
        "a saved profile waits for the repository it is applied to"
    );
    set(cx, |app| app.loading = None);
    assert_eq!(
        refused(cx),
        with(&[WORK, PERSONAL], "repository unavailable"),
        "built again as the read ends without it, each says why beside it"
    );
    app.update(cx, |app, cx| {
        app.repository = repository;
        cx.notify();
    });
    draw(cx);
    assert_eq!(
        refused(cx),
        Vec::<String>::new(),
        "built again as the repository is read, they are offered"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
}

/// A marked tab's saved profiles say the mark beside them, the open menu
/// built again as the mark lands, and Apply prepares no review: the
/// repository the tab shows is the one read before the mark, whose
/// configuration Apply's review named and whose write Git core then
/// refused (security probe Q3, FU39-A). Manage profiles… says the mark
/// too, as the palette's Manage Git profiles… does, and no route opens the
/// manager, whose Apply… names that repository (decided 2026-11-11: an
/// entry is offered only where it can act); New profile…, which writes no
/// repository, stays offered.
#[gpui::test]
async fn a_marked_tab_s_saved_profiles_say_the_mark(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    use_store(&app, cx, two_profiles()).await;
    open_menu(&app, cx).await;
    assert_eq!(refused(cx), Vec::<String>::new(), "offered before the mark");
    set(&app, cx, |app| {
        let index = app.repository_tabs.active.expect("a tab");
        app.repository_tabs.tabs[index].error = Some("Repository unavailable".into());
    });
    assert_eq!(
        refused(cx),
        [
            "Work · qa@work.example, repository unavailable",
            "Personal · qa@home.example, repository unavailable",
            "Manage profiles…, repository unavailable",
        ],
        "built again as the mark lands, each says it beside it, and Manage \
         profiles… too, as the palette's Manage Git profiles… does"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    // Whatever route asks for it, the manager does not open.
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_profiles(window, cx)));
    settle(&app, cx).await;
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "the manager opens on no route"
    );
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            crate::profiles::apply_profile(app, window, cx)
        })
    });
    settle(&app, cx).await;
    assert_eq!(
        app.read_with(cx, |app, _| app.operation_busy),
        None,
        "no review is prepared"
    );
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "no review opens"
    );
}

/// The bounds of the Settings page's element with debug selector
/// `selector`, read from a frame that builds the page again: GPUI records
/// an element's bounds only in a frame that paints it, and a frame that
/// replays the cached page (`GitTurtle::settings_page_element`) records
/// none of its elements'.
fn page_bounds(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    selector: &'static str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    let page = app.read_with(cx, |app, _| app.settings_page_for_test());
    cx.update(|window, cx| {
        page.update(cx, |_, cx| cx.notify());
        window.draw(cx).clear(cx);
    });
    cx.debug_bounds(selector)
}

/// The bounds of the Settings page's control with debug selector
/// `selector` once the page is scrolled to it as a person scrolls, by a
/// wheel step over the page of the least that shows it whole with a ring's
/// room, asserted drawn whole inside the window and the page's scroll view,
/// where a click reaches it. A click outside them reaches nothing, so that
/// it did nothing proves nothing.
fn wheeled_into_view(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    selector: &'static str,
) -> gpui::Bounds<gpui::Pixels> {
    let room = gpui::px(4.);
    let view = page_bounds(app, cx, "settings-scroll").expect("Settings is drawn");
    let control = page_bounds(app, cx, selector).expect(selector);
    let step = if control.bottom() + room > view.bottom() {
        view.bottom() - control.bottom() - room
    } else if control.top() - room < view.top() {
        view.top() - control.top() + room
    } else {
        gpui::px(0.)
    };
    if step != gpui::px(0.) {
        // Over the page's own padding, clear of the Your themes list, which
        // takes a step while it can move.
        let over = gpui::point(view.left() + gpui::px(12.), view.center().y);
        cx.simulate_mouse_move(over, None, gpui::Modifiers::default());
        cx.simulate_event(gpui::ScrollEvent {
            position: over,
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), step)),
            ..Default::default()
        });
    }
    let view = page_bounds(app, cx, "settings-scroll").expect("Settings is drawn");
    let control = page_bounds(app, cx, selector).expect(selector);
    let window = gpui::Bounds::new(
        gpui::point(gpui::px(0.), gpui::px(0.)),
        cx.update(|window, _| window.viewport_size()),
    );
    let whole_inside = |outer: gpui::Bounds<gpui::Pixels>| {
        control.left() >= outer.left()
            && control.right() <= outer.right()
            && control.top() >= outer.top()
            && control.bottom() <= outer.bottom()
    };
    assert!(
        whole_inside(window) && whole_inside(view),
        "{selector} drawn whole inside the window, {window:?}, and the page's \
         view, {view:?}: {control:?}"
    );
    control
}

/// The name the Settings Button with element ID `key` gives assistive
/// technology as the window last drew it, and whether it is announced
/// unavailable (`DrawnButton::node_recorded`).
fn drawn_button(key: &str) -> Option<(Option<String>, bool)> {
    crate::native_accessibility::written::get(key).map(|written| {
        (
            written.node.label().map(str::to_owned),
            written.node.is_disabled(),
        )
    })
}

/// Settings' Manage named profiles… on a marked tab is refused with the
/// mark's reason, as the profile picker's Manage profiles… is: announced
/// unavailable and dimmed, its tooltip saying "Repository unavailable",
/// while no route opens the manager there (`open_profiles`). It was drawn
/// enabled, and a click opened nothing and said nothing. Offered again as
/// the mark clears.
#[gpui::test]
async fn settings_manage_named_profiles_says_the_mark(cx: &mut TestAppContext) {
    use crate::workspace::refusals::{self, Refusal};
    const KEY: &str = "settings-manage-profiles";
    let (_fixture, app, cx) = history_window(cx).await;
    use_store(&app, cx, two_profiles()).await;
    hold_quiet_reads_for_test(&app, cx);
    // The notes outlive a seed's app on its thread; only this one's count.
    crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow_mut().clear());
    refusals::forget();
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let tooltip =
        || crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow().get(KEY).cloned());
    let offered = Some(Refusal {
        announced: false,
        dimmed: false,
    });
    assert_eq!(refusals::get(KEY), offered, "offered before the mark");
    assert_eq!(tooltip(), None, "no reason to give");
    assert_eq!(
        drawn_button(KEY),
        Some((Some("Manage named profiles…".into()), false)),
        "drawn under its own name"
    );
    set(&app, cx, |app| {
        let index = app.repository_tabs.active.expect("a tab");
        app.repository_tabs.tabs[index].error = Some("Repository unavailable".into());
    });
    assert_eq!(
        refusals::get(KEY),
        Some(Refusal {
            announced: true,
            dimmed: true,
        }),
        "refused and dimmed on the marked tab"
    );
    assert_eq!(
        tooltip().as_deref(),
        Some("Manage named profiles… · Repository unavailable"),
        "its tooltip says why"
    );
    assert_eq!(
        drawn_button(KEY),
        Some((
            Some("Manage named profiles…: Repository unavailable".into()),
            true
        )),
        "drawn unavailable, its accessible name saying why, as Fetch's does: \
         out of the Tab order, its tooltip shows only under the pointer"
    );
    set(&app, cx, |app| {
        let index = app.repository_tabs.active.expect("a tab");
        app.repository_tabs.tabs[index].error = None;
    });
    assert_eq!(refusals::get(KEY), offered, "offered as the mark clears");
    assert_eq!(
        drawn_button(KEY),
        Some((Some("Manage named profiles…".into()), false)),
        "its own name again"
    );
}

/// Settings' Git identity on a marked tab says the mark and nothing from
/// before it, as the bar and the navigation do there: its line reads the
/// mark's reason and its way back in place of "Current identity: …", the
/// identity read before the mark, and Name and Email are unavailable as
/// while a write runs, refused and dimmed, so nothing typed there waits for
/// a save no route makes. They took a Name and Email the dimmed Save could
/// not write, and nothing on the page said why. Both come back as the mark
/// clears. The fields are read in the frame the mark lands in, before one
/// is focused, which builds the page again, so a page the mark alone does
/// not build again shows here.
#[gpui::test]
async fn settings_identity_fields_and_line_say_the_mark(cx: &mut TestAppContext) {
    use crate::branch_actions::{UNAVAILABLE_REASON, UNAVAILABLE_WAY_BACK};
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    set(&app, cx, |app| {
        app.profile = Some(GitProfile {
            name: "Fixture Person".into(),
            email: "person@example.invalid".into(),
            ..GitProfile::default()
        })
    });
    let (name, email) = app.read_with(cx, |app, _| {
        (app.identity_name_for_test(), app.identity_email_for_test())
    });
    let current =
        "settings-identity-line: Current identity: Fixture Person <person@example.invalid>";
    let marked: &'static str =
        format!("settings-identity-line: {UNAVAILABLE_REASON} · {UNAVAILABLE_WAY_BACK}").leak();
    // Whether each field, as the page last drew it, takes typing and looks
    // it: the state the drawn Input gave the field.
    let fields = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            [&name, &email].map(|field| {
                let field = field.read(cx);
                (field.is_editable(), field.presentation().is_disabled())
            })
        })
    };
    // Typed into `field`, focused, as a person types: what it holds after.
    let typed = |field: &Entity<InputState>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| field.read(cx).focus_handle(cx).focus(window, cx));
        draw(cx);
        let before = cx.read(|cx| field.read(cx).value());
        cx.simulate_input("x");
        draw(cx);
        let after = cx.read(|cx| field.read(cx).value());
        (before, after)
    };
    assert!(
        page_bounds(&app, cx, current).is_some(),
        "the identity read"
    );
    assert_eq!(fields(cx), [(true, false); 2], "offered before the mark");
    // Typing in both, which the mark leaves as typed: a field holding the
    // identity read is emptied as the mark lands, which builds the page
    // again whatever else does.
    for field in [&name, &email] {
        typed(field, cx);
    }

    set(&app, cx, |app| {
        let index = app.repository_tabs.active.expect("a tab");
        app.repository_tabs.tabs[index].error = Some("Repository unavailable".into());
    });
    assert_eq!(
        fields(cx),
        [(false, true); 2],
        "refused and dimmed as the mark lands, as while a write runs"
    );
    for (field, which) in [(&name, "Name"), (&email, "Email")] {
        let (before, after) = typed(field, cx);
        assert_eq!(after, before, "{which}: nothing typed is taken");
    }
    assert!(
        page_bounds(&app, cx, marked).is_some(),
        "the line says the mark and its way back"
    );
    assert!(
        page_bounds(&app, cx, current).is_none(),
        "nothing from before the mark"
    );

    set(&app, cx, |app| {
        let index = app.repository_tabs.active.expect("a tab");
        app.repository_tabs.tabs[index].error = None;
    });
    assert_eq!(fields(cx), [(true, false); 2], "offered as the mark clears");
    assert!(page_bounds(&app, cx, marked).is_none(), "the mark is gone");
    // Cleared in place, as by a Refresh's snapshot, the tab holds only what
    // was read before the mark until a read lands: the line names no
    // identity until one does.
    assert!(
        page_bounds(&app, cx, current).is_none()
            && page_bounds(&app, cx, IDENTITY_NOT_LOADED).is_some(),
        "no identity until a read after the mark lands"
    );
    land_identity(
        &app,
        cx,
        ("Fixture Person", "person@example.invalid"),
        false,
    );
    assert!(
        page_bounds(&app, cx, current).is_some(),
        "the identity again"
    );
}

/// Marks the shown tab, as a read that cannot open its repository does,
/// and draws.
fn mark(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, error: Option<&str>) {
    set(app, cx, |app| {
        let index = app.repository_tabs.active.expect("a tab");
        app.repository_tabs.tabs[index].error = error.map(Into::into);
    });
}

/// What a Settings field gives assistive technology as the window draws
/// it: its name, its value, its placeholder, and whether it takes typing
/// and looks unavailable.
type DrawnField = (Option<String>, String, Option<String>, (bool, bool));

/// What Settings' Name and Email give assistive technology as the window
/// draws them ([`DrawnField`]).
fn drawn_fields(cx: &mut VisualTestContext, fields: &[Entity<InputState>; 2]) -> [DrawnField; 2] {
    draw(cx);
    fields.clone().map(|field| {
        let node = crate::views::tests::input_node(cx, &field);
        let value = cx.update(|_, cx| {
            field.update(cx, |state, _| {
                state
                    .accessible_text(true)
                    .map(|text| text.value().to_string())
            })
        });
        let state = cx.read(|cx| {
            let field = field.read(cx);
            (field.is_editable(), field.presentation().is_disabled())
        });
        (
            node.label().map(str::to_owned),
            value.unwrap_or_default(),
            node.placeholder().map(str::to_owned),
            state,
        )
    })
}

/// The accessible descriptions Settings' Name and Email give as the window
/// draws them.
fn drawn_descriptions(
    cx: &mut VisualTestContext,
    fields: &[Entity<InputState>; 2],
) -> [Option<String>; 2] {
    draw(cx);
    fields.clone().map(|field| {
        crate::views::tests::input_node(cx, &field)
            .description()
            .map(str::to_owned)
    })
}

/// Settings' Name and Email on a marked tab draw nothing read before the
/// mark: a field holding the identity read from the repository is drawn
/// empty with its placeholder, as assistive technology reads it too,
/// dimmed and refused, and neither the signing line nor the note on where
/// a save goes, which that read chose, is drawn, while each field says the
/// mark and its way back as its description, as the line does. They drew
/// the identity read before the mark, dimmed, under a line saying the
/// repository was unavailable, and were announced with nothing to say why
/// (decided 2026-11-14). A Refresh that clears the mark shows what its
/// read finds in both fields, the signing line and the note.
#[gpui::test]
async fn settings_identity_fields_draw_no_value_read_before_the_mark(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    set(&app, cx, |app| {
        app.profile = Some(GitProfile {
            name: "Fixture Person".into(),
            email: "person@example.invalid".into(),
            signing: true,
            ..GitProfile::default()
        })
    });
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    let field = |name: &str, value: &str, placeholder: &str, state| {
        (
            Some(name.to_owned()),
            value.to_owned(),
            Some(placeholder.to_owned()),
            state,
        )
    };
    const SIGNED: &str = "settings-signing-line: Commit signing is enabled in Git. \
                          GitTurtle uses your configured signer.";
    const UNSIGNED: &str = "settings-signing-line: Commit signing is off in Git. \
                            Your existing Git signing configuration is preserved.";
    const UNREAD: &str = "settings-signing-line: Signing follows your existing Git configuration.";
    const SHARED: &str = "settings-identity-note: Name and email are saved to repository Git \
                          configuration shared by linked worktrees. Global identity is preserved.";
    const PRIVATE: &str = "settings-identity-note: Name and email are saved only to this \
                           worktree’s existing private Git configuration.";
    let line = format!(
        "{} · {}",
        crate::branch_actions::UNAVAILABLE_REASON,
        crate::branch_actions::UNAVAILABLE_WAY_BACK
    );
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "Fixture Person", "Your name", (true, false)),
            field(
                "Email",
                "person@example.invalid",
                "you@example.com",
                (true, false)
            ),
        ],
        "the identity read, offered"
    );
    assert!(
        page_bounds(&app, cx, SIGNED).is_some(),
        "the read's signing line"
    );
    assert!(page_bounds(&app, cx, SHARED).is_some(), "the read's note");
    assert_eq!(
        drawn_descriptions(cx, &fields),
        [None, None],
        "no description"
    );

    mark(&app, cx, Some("Repository unavailable"));
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "", "Your name", (false, true)),
            field("Email", "", "you@example.com", (false, true)),
        ],
        "empty with their placeholders, refused and dimmed"
    );
    for line in [SIGNED, UNSIGNED, UNREAD, SHARED, PRIVATE] {
        assert!(
            page_bounds(&app, cx, line).is_none(),
            "no signing line or note: {line}"
        );
    }
    assert_eq!(
        drawn_descriptions(cx, &fields),
        [Some(line.clone()), Some(line)],
        "each says the mark and its way back"
    );

    // The repository's own identity, which the Refresh's read finds.
    for (key, value) in [
        ("user.name", "Read Again"),
        ("user.email", "again@example.invalid"),
    ] {
        let status = std::process::Command::new("git")
            .args(["config", "--local", key, value])
            .current_dir(&root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success(), "{key} configured");
    }
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
    settle(&app, cx).await;
    assert!(
        !app.read_with(cx, |app, _| app.shown_tab_unavailable()),
        "the Refresh clears the mark"
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "Read Again", "Your name", (true, false)),
            field(
                "Email",
                "again@example.invalid",
                "you@example.com",
                (true, false)
            ),
        ],
        "what the read finds, offered"
    );
    assert!(
        page_bounds(&app, cx, UNSIGNED).is_some(),
        "the read's signing line"
    );
    assert!(page_bounds(&app, cx, SHARED).is_some(), "the read's note");
    assert_eq!(
        drawn_descriptions(cx, &fields),
        [None, None],
        "no description again"
    );
}

/// Text typed into Settings' Name and not saved is the person's: on a
/// marked tab it stays as typed, dimmed and refused, while Email, holding
/// the identity read before the mark, is drawn empty. As the mark clears
/// with a read that finds another identity the typing is still there,
/// while Email takes what that read finds, never the address read before
/// the mark (decided 2026-11-14): it went back to that address, kept with
/// the edit as an unsaved edit keeps both fields.
#[gpui::test]
async fn settings_identity_typing_stays_as_typed_on_a_marked_tab(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    set(&app, cx, |app| {
        app.profile = Some(GitProfile {
            name: "Fixture Person".into(),
            email: "person@example.invalid".into(),
            ..GitProfile::default()
        })
    });
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    cx.update(|window, cx| fields[0].read(cx).focus_handle(cx).focus(window, cx));
    draw(cx);
    cx.simulate_input(" Typed");
    let field = |name: &str, value: &str, placeholder: &str, state| {
        (
            Some(name.to_owned()),
            value.to_owned(),
            Some(placeholder.to_owned()),
            state,
        )
    };
    assert_eq!(
        drawn_fields(cx, &fields)[0],
        field("Name", "Fixture Person Typed", "Your name", (true, false)),
        "typed"
    );

    mark(&app, cx, Some("Repository unavailable"));
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "Fixture Person Typed", "Your name", (false, true)),
            field("Email", "", "you@example.com", (false, true)),
        ],
        "the typing as typed and the read Email empty, both refused and dimmed"
    );

    land_identity(&app, cx, ("Read Again", "again@example.invalid"), true);
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "Fixture Person Typed", "Your name", (true, false)),
            field(
                "Email",
                "again@example.invalid",
                "you@example.com",
                (true, false)
            ),
        ],
        "the typing kept as the mark clears, Email what the read finds"
    );
}

/// A value the fields were filled with from the repository is read, not
/// typed, though a later read found another identity: an unsaved edit of
/// Name keeps both fields through that read (`DraftState::identity_update`),
/// so Email still holds the address read before it, and on a marked tab
/// that address is drawn empty with its placeholder while Name keeps its
/// typing (`DraftState::was_read`). Email drew the earlier address, read
/// before the mark, matching neither the latest read nor anything typed.
#[gpui::test]
async fn settings_identity_fields_draw_no_value_an_earlier_read_filled(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let read = |app: &mut GitTurtle, name: &str, email: &str| {
        app.profile = Some(GitProfile {
            name: name.into(),
            email: email.into(),
            ..GitProfile::default()
        })
    };
    set(&app, cx, |app| read(app, "Bob", "bob@example.invalid"));
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    let field = |name: &str, value: &str, placeholder: &str, state| {
        (
            Some(name.to_owned()),
            value.to_owned(),
            Some(placeholder.to_owned()),
            state,
        )
    };
    cx.update(|window, cx| fields[0].read(cx).focus_handle(cx).focus(window, cx));
    draw(cx);
    cx.simulate_input(" X");
    // A read after an outside change finds another identity; the unsaved
    // edit keeps both fields as they were.
    set(&app, cx, |app| read(app, "Carol", "carol@example.invalid"));
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "Bob X", "Your name", (true, false)),
            field(
                "Email",
                "bob@example.invalid",
                "you@example.com",
                (true, false)
            ),
        ],
        "the edit and the address it kept"
    );

    mark(&app, cx, Some("Repository unavailable"));
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "Bob X", "Your name", (false, true)),
            field("Email", "", "you@example.com", (false, true)),
        ],
        "the typing as typed, the address read before the mark empty"
    );
}

/// A read on a marked tab finding no identity leaves the read that clears
/// the mark nothing to take for an edit: Name and Email, drawn empty on the
/// marked tab, show what that read finds, Save is not offered for it, and a
/// click on Save asks for no write. The empty read matched the emptied
/// fields, so the identity read before the mark, given back as the mark
/// cleared, was taken for an unsaved edit: the fields drew it, Save was
/// offered, and a click on it wrote it back to the repository (the
/// security re-check of FU44-D8).
#[gpui::test]
async fn settings_identity_an_empty_read_on_a_marked_tab_gives_nothing_back(
    cx: &mut TestAppContext,
) {
    use crate::workspace::refusals::{self, Refusal};
    const SAVE: &str = "save-repository-identity";
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    refusals::forget();
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    let field = |name: &str, value: &str, placeholder: &str, state| {
        (
            Some(name.to_owned()),
            value.to_owned(),
            Some(placeholder.to_owned()),
            state,
        )
    };
    mark(&app, cx, Some("Repository unavailable"));
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "", "Your name", (false, true)),
            field("Email", "", "you@example.com", (false, true)),
        ],
        "the identity read before the mark drawn empty"
    );
    // A read of the marked tab, its `.git` back or replaced, finding no
    // identity; then the mark clears with a read finding another.
    land_identity(&app, cx, ("", ""), false);
    land_identity(&app, cx, ("New Read", "new@example.invalid"), true);
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "New Read", "Your name", (true, false)),
            field(
                "Email",
                "new@example.invalid",
                "you@example.com",
                (true, false)
            ),
        ],
        "what the read that clears the mark finds"
    );
    assert_eq!(
        refusals::get(SAVE),
        Some(Refusal {
            announced: true,
            dimmed: true,
        }),
        "Save not offered for what the read finds"
    );
    let writes = std::rc::Rc::new(std::cell::Cell::new(0_usize));
    let _counting = cx.update(|_, cx| {
        let writes = writes.clone();
        let mut was = false;
        cx.observe(&app, move |app, cx| {
            let now = app.read(cx).operation_busy == Some("Saving repository identity…");
            writes.set(writes.get() + usize::from(now && !was));
            was = now;
        })
    });
    let save = wheeled_into_view(&app, cx, SAVE);
    cx.simulate_click(save.center(), gpui::Modifiers::default());
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    assert_eq!(writes.get(), 0, "a click on Save asks for no write");
}

/// Lands a working-state read of the shown repository that finds `name`
/// and `email` as its Git identity, as the status read a Refresh asks for
/// lands (`GitTurtle::apply_worktree_state`), and draws; where
/// `clears_the_mark`, the shown tab's mark clears in the same update, as a
/// quiet read that finds the repository readable again lands its read and
/// clears it (`GitTurtle::tab_found_readable`).
fn land_identity(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    (name, email): (&str, &str),
    clears_the_mark: bool,
) {
    let repository = app.read_with(cx, |app, _| app.repository.clone().expect("a repository"));
    let mut state = crate::worker::WorkingState::read(&repository).expect("the working state");
    state.profile = Ok(GitProfile {
        name: name.into(),
        email: email.into(),
        ..GitProfile::default()
    });
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let requested = app.request_status_read();
            app.apply_worktree_state(state, false, false, requested, window, cx);
            if clears_the_mark {
                let index = app.repository_tabs.active.expect("a tab");
                app.repository_tabs.tabs[index].error = None;
            }
            cx.notify();
        })
    });
    draw(cx);
}

/// Settings opened after a Refresh's snapshot has cleared the tab's mark in
/// place, before that Refresh's status read lands, draws Name and Email
/// empty with their placeholders, offered, and they take nothing until a
/// read landing after the mark does: the window holds the identity read
/// before the mark until then, as an Open in place keeps what it read
/// (`GitTurtle::tab_snapshot_accepted`), and the fields drew it (FU44-D9's
/// code re-check: Settings opened between the snapshot and the status
/// read). Typing there meanwhile stays as typed as that read lands, Email
/// taking what it finds, and Save writes only that, the typing and the new
/// read's address, never the identity read before the mark (decided
/// 2026-11-14: nothing from before the mark). `shown_before`: whether
/// Settings had drawn the identity read before the mark, which the fields
/// then held while History was shown.
async fn settings_identity_waits_after_an_in_place_clear(
    cx: &mut TestAppContext,
    shown_before: bool,
) {
    const SAVE: &str = "save-repository-identity";
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let configured = || {
        ["user.name", "user.email"].map(|key| {
            let output = std::process::Command::new("git")
                .args(["config", "--local", "--get", key])
                .current_dir(&root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        })
    };
    let field = |name: &str, value: &str, placeholder: &str| {
        (
            Some(name.to_owned()),
            value.to_owned(),
            Some(placeholder.to_owned()),
            (true, false),
        )
    };
    let read_fields = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| {
            [app.identity_name_for_test(), app.identity_email_for_test()]
        })
    };
    land_identity(&app, cx, ("Before Mark", "before@example.invalid"), false);
    if shown_before {
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        settle(&app, cx).await;
        let fields = read_fields(cx);
        assert_eq!(
            drawn_fields(cx, &fields),
            [
                field("Name", "Before Mark", "Your name"),
                field("Email", "before@example.invalid", "you@example.com"),
            ],
            "the identity read before the mark"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        settle(&app, cx).await;
    }
    // On History the read that marks the tab lands, then a Refresh's
    // snapshot clears the mark in place, its status read still to land.
    mark(&app, cx, Some("Repository unavailable"));
    mark(&app, cx, None);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = read_fields(cx);
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "", "Your name"),
            field("Email", "", "you@example.com"),
        ],
        "empty with their placeholders, offered, until a read after the mark lands"
    );
    cx.update(|window, cx| fields[0].read(cx).focus_handle(cx).focus(window, cx));
    draw(cx);
    cx.simulate_input("Typed");
    land_identity(&app, cx, ("After Mark", "after@example.invalid"), false);
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "Typed", "Your name"),
            field("Email", "after@example.invalid", "you@example.com"),
        ],
        "the typing as typed, Email what the read after the mark finds"
    );
    let writes = std::rc::Rc::new(std::cell::Cell::new(0_usize));
    let _counting = cx.update(|_, cx| {
        let writes = writes.clone();
        let mut was = false;
        cx.observe(&app, move |app, cx| {
            let now = app.read(cx).operation_busy == Some("Saving repository identity…");
            writes.set(writes.get() + usize::from(now && !was));
            was = now;
        })
    });
    let save = wheeled_into_view(&app, cx, SAVE);
    cx.simulate_click(save.center(), gpui::Modifiers::default());
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    assert_eq!(writes.get(), 1, "Save asks for one identity write");
    assert_eq!(
        configured(),
        ["Typed", "after@example.invalid"],
        "it writes the typing and the read's address, nothing from before the mark"
    );
}

#[gpui::test]
async fn settings_identity_opened_first_after_an_in_place_clear_waits_for_its_read(
    cx: &mut TestAppContext,
) {
    settings_identity_waits_after_an_in_place_clear(cx, false).await;
}

#[gpui::test]
async fn settings_identity_opened_again_after_an_in_place_clear_waits_for_its_read(
    cx: &mut TestAppContext,
) {
    settings_identity_waits_after_an_in_place_clear(cx, true).await;
}

/// Settings shown as the tab's mark lands and as a Refresh's snapshot
/// clears it in place keeps Name and Email empty, offered, until that
/// Refresh's status read lands, and Reset edits meanwhile restores nothing
/// read before the mark, leaving both to wait for that read: the identity
/// read before the mark came back into both as the mark cleared (FU44-D9's
/// code re-check), and Reset edits filled it, so either, kept as an edit
/// once the read landed, was what Save wrote. The read after the mark
/// fills both.
#[gpui::test]
async fn settings_identity_shown_through_an_in_place_clear_waits_for_its_read(
    cx: &mut TestAppContext,
) {
    const RESET: &str = "use-current-identity";
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    land_identity(&app, cx, ("Before Mark", "before@example.invalid"), false);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    let field = |name: &str, value: &str, placeholder: &str, state| {
        (
            Some(name.to_owned()),
            value.to_owned(),
            Some(placeholder.to_owned()),
            state,
        )
    };
    let empty = |state| {
        [
            field("Name", "", "Your name", state),
            field("Email", "", "you@example.com", state),
        ]
    };
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "Before Mark", "Your name", (true, false)),
            field(
                "Email",
                "before@example.invalid",
                "you@example.com",
                (true, false)
            ),
        ],
        "the identity read before the mark"
    );
    mark(&app, cx, Some("Repository unavailable"));
    assert_eq!(drawn_fields(cx, &fields), empty((false, true)), "marked");
    mark(&app, cx, None);
    assert_eq!(
        drawn_fields(cx, &fields),
        empty((true, false)),
        "the mark cleared in place, no read after it yet"
    );
    let reset = wheeled_into_view(&app, cx, RESET);
    cx.simulate_click(reset.center(), gpui::Modifiers::default());
    settle(&app, cx).await;
    assert_eq!(
        drawn_fields(cx, &fields),
        empty((true, false)),
        "Reset edits restores nothing read before the mark"
    );
    land_identity(&app, cx, ("After Mark", "after@example.invalid"), false);
    assert_eq!(
        drawn_fields(cx, &fields),
        [
            field("Name", "After Mark", "Your name", (true, false)),
            field(
                "Email",
                "after@example.invalid",
                "you@example.com",
                (true, false)
            ),
        ],
        "what the read after the mark finds"
    );
}

const SAVE_IDENTITY: &str = "save-repository-identity";
const RESET_IDENTITY: &str = "use-current-identity";
/// What Settings' identity line and signing line read with no identity
/// loaded, and the two notes on where a save goes, neither drawn then.
const IDENTITY_NOT_LOADED: &str = "settings-identity-line: Git identity has not been loaded yet.";
const SIGNING_NOT_LOADED: &str =
    "settings-signing-line: Signing follows your existing Git configuration.";
const IDENTITY_NOTES: [&str; 2] = [
    "settings-identity-note: Name and email are saved to repository Git configuration shared by \
     linked worktrees. Global identity is preserved.",
    "settings-identity-note: Name and email are saved only to this worktree’s existing private \
     Git configuration.",
];

/// The local `user.name` and `user.email` in the configuration of the
/// repository at `root`, as Git reads them.
fn configured_identity(root: &std::path::Path) -> (String, String) {
    let get = |key: &str| {
        let output = std::process::Command::new("git")
            .args(["config", "--local", "--get", key])
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    (get("user.name"), get("user.email"))
}

/// Counts the identity writes the window asks for while the returned
/// subscription lives, each as it is asked, before the operation executor
/// runs it.
fn identity_writes(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
) -> (std::rc::Rc<std::cell::Cell<usize>>, gpui::Subscription) {
    let writes = std::rc::Rc::new(std::cell::Cell::new(0_usize));
    let counting = cx.update(|_, cx| {
        let writes = writes.clone();
        let mut was = false;
        cx.observe(app, move |app, cx| {
            let now = app.read(cx).operation_busy == Some("Saving repository identity…");
            writes.set(writes.get() + usize::from(now && !was));
            was = now;
        })
    });
    (writes, counting)
}

/// What Settings' Name and Email hold.
fn identity_values(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> (String, String) {
    app.read_with(cx, |app, cx| {
        (
            app.identity_name_for_test().read(cx).value().to_string(),
            app.identity_email_for_test().read(cx).value().to_string(),
        )
    })
}

/// Types `text` into `field`, focused, as a person types.
fn type_into(cx: &mut VisualTestContext, field: &Entity<InputState>, text: &str) {
    cx.update(|window, cx| field.read(cx).focus_handle(cx).focus(window, cx));
    draw(cx);
    cx.simulate_input(text);
    draw(cx);
}

/// Shows Settings and draws, awaiting no read: a status read a test holds
/// on the operation executor never lands for `settle`.
fn show_settings_held(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    draw(cx);
}

/// Lands the reads the window awaits as its own task, an Open's snapshot
/// and the changed files it goes on to, leaving the status read to
/// whatever holds the operation executor.
async fn land_window_reads(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    for _ in 0..8 {
        cx.executor().run_until_parked();
        let Some(task) = cx.update(|_, cx| app.update(cx, |app, _| app.task.take())) else {
            break;
        };
        task.await;
    }
    draw(cx);
}

/// Lets the quiet reads asked for run and land on the repository page,
/// then holds them again.
fn land_quiet_reads(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.automatic.hold_quiet_reads(false);
            app.try_automatic_refresh(window, cx);
        })
    });
    for _ in 0..16 {
        app.read_with(cx, |app, _| app.worker.wait_idle());
        cx.executor().run_until_parked();
        if app.read_with(cx, |app, _| {
            !app.automatic.quiet_read_requested() && app.automatic.replies_landed()
        }) {
            break;
        }
    }
    hold_quiet_reads_for_test(app, cx);
    assert!(
        !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()),
        "the quiet reads landed"
    );
    draw(cx);
}

/// Asserts Settings shows no identity loaded, `at`: the identity line
/// names none, the signing line claims nothing, no note says where a save
/// goes, and Save repository identity and Reset edits are refused and
/// dimmed with `reason` in their tooltips and accessible names, as Fetch
/// gives it.
fn assert_no_identity_loaded(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    reason: &str,
    at: &str,
) {
    use crate::workspace::refusals::{self, Refusal};
    crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow_mut().clear());
    assert!(
        page_bounds(app, cx, IDENTITY_NOT_LOADED).is_some(),
        "{at}: the identity line names none"
    );
    assert!(
        page_bounds(app, cx, SIGNING_NOT_LOADED).is_some(),
        "{at}: the signing line claims nothing"
    );
    for note in IDENTITY_NOTES {
        assert!(page_bounds(app, cx, note).is_none(), "{at}: no {note}");
    }
    for (key, label, purpose) in [
        (
            SAVE_IDENTITY,
            "Save repository identity",
            "Save repository identity",
        ),
        (
            RESET_IDENTITY,
            "Reset edits",
            "Restore the repository’s current name and email",
        ),
    ] {
        assert_eq!(
            refusals::get(key),
            Some(Refusal {
                announced: true,
                dimmed: true,
            }),
            "{at}: {key} refused and dimmed"
        );
        assert_eq!(
            crate::views::CONTROL_TOOLTIPS
                .with(|tooltips| tooltips.borrow().get(key).cloned())
                .as_deref(),
            Some(format!("{purpose} · {reason}").as_str()),
            "{at}: {key}'s tooltip says why"
        );
        assert_eq!(
            drawn_button(key),
            Some((Some(format!("{label}: {reason}")), true)),
            "{at}: {key} drawn unavailable, its accessible name saying why"
        );
    }
}

/// Presses Save repository identity and Reset edits where the page shows
/// them, then calls each handler as a key or a stale press reaches it.
/// Nothing here waits on the operation executor, which a test may hold.
fn press_identity_actions(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    for key in [SAVE_IDENTITY, RESET_IDENTITY] {
        let control = wheeled_into_view(app, cx, key);
        cx.simulate_click(control.center(), gpui::Modifiers::default());
        draw(cx);
    }
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.save_identity(window, cx);
            app.reset_identity_edits(window, cx);
        })
    });
    draw(cx);
}

/// Clicks Save repository identity where the page shows it and lands the
/// write it asks for.
async fn save_identity_by_click(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    let save = wheeled_into_view(app, cx, SAVE_IDENTITY);
    cx.simulate_click(save.center(), gpui::Modifiers::default());
    app.read_with(cx, |app, _| app.operations.drain());
    settle(app, cx).await;
}

/// Marks the shown tab as its repository's own read does: its `.git` moved
/// to `away`, and the read that finds it gone.
async fn mark_for_real(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    root: &std::path::Path,
    away: &std::path::Path,
) {
    std::fs::rename(root.join(".git"), away).unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.tab_repository_gone(window, cx)));
    settle(app, cx).await;
    assert!(
        app.read_with(cx, |app, _| app.shown_tab_unavailable()),
        "the read that finds the repository gone marks the tab"
    );
}

/// A Refresh of the shown, marked tab whose snapshot lands and clears the
/// mark in place while its status read waits behind a hold of the operation
/// executor, which the caller keeps.
async fn refresh_to_its_snapshot(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
    land_window_reads(app, cx).await;
    assert!(
        app.read_with(cx, |app, _| !app.shown_tab_unavailable()
            && app.status_task.is_some()),
        "the snapshot clears the mark, its status read still to land"
    );
}

/// A window with two tabs, quiet reads held: the fixture, whose identity is
/// "Fixture", shown, and another repository, "Other Name", retained.
/// Returns the fixture's root and the other's.
async fn two_identities(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<GitTurtle>,
    &mut VisualTestContext,
    [std::path::PathBuf; 2],
) {
    let (fixture, app, cx) = history_window(cx).await;
    let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let other = other_identity(&fixture);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_repository_tab(other.clone(), window, cx)
        })
    });
    settle(&app, cx).await;
    settle(&app, cx).await;
    switch_tab(&app, cx, 0).await;
    hold_quiet_reads_for_test(&app, cx);
    draw(cx);
    (fixture, app, cx, [root, other])
}

/// Another repository beside the fixture's, its identity "Other Name".
fn other_identity(fixture: &tempfile::TempDir) -> std::path::PathBuf {
    let other = fixture_repository(fixture.path().join("other"))
        .path()
        .to_owned();
    fixture_git(&other, &["config", "user.name", "Other Name"]);
    fixture_git(&other, &["config", "user.email", "other@example.invalid"]);
    other
}

async fn switch_tab(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, index: usize) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(index, window, cx)));
    settle(app, cx).await;
}

/// Settings on a tab whose repository has loaded but whose identity has not
/// been read yet shows none, and nothing of the repository shown before:
/// with Settings shown on one repository, a Name typed there, and another
/// opened in a new tab, its status read held, Name and Email are empty with
/// their placeholders, the identity line and the signing line read their
/// not-loaded sentences, no note says where a save goes, and Save
/// repository identity and Reset edits are refused, "Reading the working
/// tree…" in their tooltips and accessible names, by a click and by their
/// handlers. The fields kept the first repository's Name and Email, and
/// Save, offered whenever no identity was held, wrote them into the
/// repository shown (decided 2026-11-15: Settings has no identity until one
/// is read for the repository shown). As the read lands, the fields take
/// its identity; where it fails (`read_fails`) they stay empty and the
/// actions say "Working tree not read · Refresh". Nothing is written to the
/// repository's configuration either way.
async fn settings_identity_waits_on_a_cold_tab(cx: &mut TestAppContext, read_fails: bool) {
    use crate::workspace::{NOT_READ_REASON, READING_WORKING_TREE};
    let (fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    let other = other_identity(&fixture);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    type_into(cx, &fields[0], " Typed");
    assert_eq!(
        identity_values(&app, cx),
        ("Fixture Typed".into(), "fixture@example.invalid".into()),
        "the first repository's identity, its Name edited"
    );

    if read_fails {
        std::fs::write(other.join(".git").join("index"), b"not an index").unwrap();
    }
    let held = app.read_with(cx, |app, _| app.operations.hold());
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_repository_tab(other.clone(), window, cx)
        })
    });
    land_window_reads(&app, cx).await;
    assert!(
        app.read_with(cx, |app, _| app.repository.is_some()
            && app.status_task.is_some()),
        "the other repository is shown, its status read still to land"
    );
    show_settings_held(&app, cx);
    let (writes, _counting) = identity_writes(&app, cx);
    let empty = [("Name", "Your name"), ("Email", "you@example.com")].map(|(name, placeholder)| {
        (
            Some(name.to_owned()),
            String::new(),
            Some(placeholder.to_owned()),
            (true, false),
        )
    });
    assert_eq!(
        drawn_fields(cx, &fields),
        empty,
        "empty with their placeholders, nothing of the repository shown before"
    );
    assert_no_identity_loaded(&app, cx, READING_WORKING_TREE, "while its status is read");
    press_identity_actions(&app, cx);
    assert_eq!(writes.get(), 0, "no press asks for a write");
    assert_eq!(
        identity_values(&app, cx),
        (String::new(), String::new()),
        "Reset edits restores nothing"
    );

    drop(held);
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    if read_fails {
        // The quiet reads the watcher asked for meanwhile fail as the
        // status read did; with none left to run, nothing reads.
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        land_quiet_reads(&app, cx);
        show_settings_held(&app, cx);
        assert_eq!(drawn_fields(cx, &fields), empty, "empty for good");
        assert_no_identity_loaded(&app, cx, NOT_READ_REASON, "after its read failed");
    } else {
        assert_eq!(
            identity_values(&app, cx),
            ("Other Name".into(), "other@example.invalid".into()),
            "the read's identity fills the empty fields"
        );
        assert!(
            page_bounds(
                &app,
                cx,
                "settings-identity-line: Current identity: Other Name <other@example.invalid>"
            )
            .is_some(),
            "and the identity line names it"
        );
        assert!(
            page_bounds(&app, cx, IDENTITY_NOTES[0]).is_some(),
            "with the note on where a save goes"
        );
    }
    press_identity_actions(&app, cx);
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    assert_eq!(writes.get(), 0, "nothing to save");
    assert_eq!(
        configured_identity(&other),
        ("Other Name".into(), "other@example.invalid".into()),
        "the repository's configuration is as it was"
    );
}

#[gpui::test]
async fn settings_identity_on_a_cold_tab_waits_for_its_read(cx: &mut TestAppContext) {
    settings_identity_waits_on_a_cold_tab(cx, false).await;
}

#[gpui::test]
async fn settings_identity_on_a_cold_tab_whose_read_fails_stays_unloaded(cx: &mut TestAppContext) {
    settings_identity_waits_on_a_cold_tab(cx, true).await;
}

/// How a tab is left and come back to while it waits for the read after
/// its mark (`settings_identity_waits_through_a_tab_switch`).
#[derive(Clone, Copy, Default)]
struct TabSwitch {
    /// Settings drew the identity read before the mark.
    opened_before: bool,
    /// Settings is shown on the other tab meanwhile.
    settings_on_other: bool,
    /// Reset edits is pressed once back.
    reset: bool,
    /// The read the tab asks for as it is shown again fails.
    read_fails: bool,
}

/// A tab left while it waits for the read after its mark, and come back
/// to, still has no identity loaded: marked, its `.git` back with another
/// address configured, a Refresh's snapshot clears the mark in place, and
/// another tab is shown before the status read lands, which drops that
/// read. Shown again, the tab holds what it read before the mark until the
/// read it asks for lands, so Settings draws Name and Email empty, says
/// "Reading the working tree…" on Save repository identity and Reset
/// edits, and a Name typed there, Reset edits and Save write nothing. The
/// note of the mark went with Settings' fields as the other repository was
/// shown, while the tab kept the identity read before the mark: the fields
/// drew it again, and Save wrote the typed Name with the address read
/// before the mark over the one configured since (the security re-check of
/// FU44-D10, S1, S2B and S2C; where Settings had drawn the identity before
/// and was not opened on the other tab the fields stayed empty for good,
/// S2A). As the read lands, Email takes the address it finds, the typed
/// Name stays, and Save writes those; where it fails (`read_fails`, S1F)
/// the fields stay as they are and the actions say "Working tree not read ·
/// Refresh".
async fn settings_identity_waits_through_a_tab_switch(cx: &mut TestAppContext, route: TabSwitch) {
    use crate::workspace::{NOT_READ_REASON, READING_WORKING_TREE};
    let (fixture, app, cx, [root, _other]) = two_identities(cx).await;
    if route.opened_before {
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        settle(&app, cx).await;
        assert_eq!(
            identity_values(&app, cx),
            ("Fixture".into(), "fixture@example.invalid".into()),
            "the identity read before the mark"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        settle(&app, cx).await;
    }
    let away = fixture.path().join("git-away");
    mark_for_real(&app, cx, &root, &away).await;
    std::fs::rename(&away, root.join(".git")).unwrap();
    fixture_git(&root, &["config", "user.email", "changed@example.invalid"]);
    let on_disk = ("Fixture".to_owned(), "changed@example.invalid".to_owned());
    let held = app.read_with(cx, |app, _| app.operations.hold());
    refresh_to_its_snapshot(&app, cx).await;
    switch_tab(&app, cx, 1).await;
    drop(held);
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    if route.settings_on_other {
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        settle(&app, cx).await;
        assert_eq!(
            identity_values(&app, cx),
            ("Other Name".into(), "other@example.invalid".into()),
            "the other tab's identity"
        );
    }
    if route.read_fails {
        std::fs::write(root.join(".git").join("index"), b"not an index").unwrap();
    }
    switch_tab(&app, cx, 0).await;
    if route.read_fails {
        land_quiet_reads(&app, cx);
    }
    let reason = if route.read_fails {
        NOT_READ_REASON
    } else {
        READING_WORKING_TREE
    };
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    let (writes, _counting) = identity_writes(&app, cx);
    assert_eq!(
        identity_values(&app, cx),
        (String::new(), String::new()),
        "nothing read before the mark, nor of the other tab"
    );
    assert_no_identity_loaded(&app, cx, reason, "back on the tab");
    if route.reset {
        let reset = wheeled_into_view(&app, cx, RESET_IDENTITY);
        cx.simulate_click(reset.center(), gpui::Modifiers::default());
        settle(&app, cx).await;
        assert_eq!(
            identity_values(&app, cx),
            (String::new(), String::new()),
            "Reset edits restores nothing read before the mark"
        );
    }
    type_into(cx, &fields[0], "Typed Name");
    assert_no_identity_loaded(&app, cx, reason, "a Name typed");
    press_identity_actions(&app, cx);
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    assert_eq!(writes.get(), 0, "no press asks for a write");
    assert_eq!(
        identity_values(&app, cx),
        ("Typed Name".into(), String::new()),
        "the typing as typed, Email still waiting"
    );
    assert_eq!(
        configured_identity(&root),
        on_disk,
        "the repository's configuration is as it was"
    );
    if route.read_fails {
        return;
    }

    // The read the tab asked for as it was shown again lands.
    cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
    land_quiet_reads(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    assert_eq!(
        identity_values(&app, cx),
        ("Typed Name".into(), "changed@example.invalid".into()),
        "Email takes what the read finds, the typed Name stays"
    );
    save_identity_by_click(&app, cx).await;
    assert_eq!(writes.get(), 1, "Save asks for one identity write");
    assert_eq!(
        configured_identity(&root),
        ("Typed Name".into(), "changed@example.invalid".into()),
        "the typing and the address read since the mark"
    );
}

#[gpui::test]
async fn settings_identity_after_a_tab_switch_shows_nothing_read_before_the_mark(
    cx: &mut TestAppContext,
) {
    let route = TabSwitch {
        settings_on_other: true,
        ..TabSwitch::default()
    };
    settings_identity_waits_through_a_tab_switch(cx, route).await;
}

#[gpui::test]
async fn settings_identity_after_a_tab_switch_stays_unloaded_where_its_read_fails(
    cx: &mut TestAppContext,
) {
    let route = TabSwitch {
        settings_on_other: true,
        read_fails: true,
        ..TabSwitch::default()
    };
    settings_identity_waits_through_a_tab_switch(cx, route).await;
}

#[gpui::test]
async fn settings_identity_shown_before_a_tab_switch_fills_as_its_read_lands(
    cx: &mut TestAppContext,
) {
    let route = TabSwitch {
        opened_before: true,
        ..TabSwitch::default()
    };
    settings_identity_waits_through_a_tab_switch(cx, route).await;
}

#[gpui::test]
async fn settings_identity_first_shown_after_a_tab_switch_waits_for_its_read(
    cx: &mut TestAppContext,
) {
    settings_identity_waits_through_a_tab_switch(cx, TabSwitch::default()).await;
}

#[gpui::test]
async fn settings_identity_reset_after_a_tab_switch_restores_nothing_read_before_the_mark(
    cx: &mut TestAppContext,
) {
    let route = TabSwitch {
        opened_before: true,
        reset: true,
        ..TabSwitch::default()
    };
    settings_identity_waits_through_a_tab_switch(cx, route).await;
}

/// Between a Refresh's snapshot clearing the tab's mark in place and its
/// status read landing, Settings shows no identity: the identity line and
/// the signing line read their not-loaded sentences, no note says where a
/// save goes, and Save repository identity and Reset edits are refused,
/// "Reading the working tree…" in their tooltips and accessible names, by a
/// click and by their handlers, whatever is typed meanwhile, the identity
/// read before the mark typed again included (`typed`; the security
/// re-check of FU44-D10, S3). The line, the signing line and the note
/// showed the read from before the mark, Reset edits was offered to
/// "Restore the repository's current name and email" and emptied both
/// fields, and Save was offered (FU44-D10's design re-check, finding A). As
/// the read lands, the lines show what it finds, a field left empty takes
/// its value, the typing stays, and Save writes those.
async fn settings_identity_waits_through_the_read_after_the_mark(
    cx: &mut TestAppContext,
    typed: (&str, &str),
) {
    use crate::workspace::READING_WORKING_TREE;
    let (fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    assert!(
        page_bounds(
            &app,
            cx,
            "settings-identity-line: Current identity: Fixture <fixture@example.invalid>"
        )
        .is_some(),
        "the identity read before the mark"
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
    settle(&app, cx).await;
    let away = fixture.path().join("git-away");
    mark_for_real(&app, cx, &root, &away).await;
    std::fs::rename(&away, root.join(".git")).unwrap();
    fixture_git(&root, &["config", "user.email", "changed@example.invalid"]);
    let held = app.read_with(cx, |app, _| app.operations.hold());
    refresh_to_its_snapshot(&app, cx).await;
    show_settings_held(&app, cx);
    let (writes, _counting) = identity_writes(&app, cx);
    assert_eq!(
        identity_values(&app, cx),
        (String::new(), String::new()),
        "nothing read before the mark"
    );
    assert_no_identity_loaded(&app, cx, READING_WORKING_TREE, "while the status is read");
    press_identity_actions(&app, cx);
    for (field, text) in fields.iter().zip([typed.0, typed.1]) {
        if !text.is_empty() {
            type_into(cx, field, text);
        }
    }
    assert_no_identity_loaded(&app, cx, READING_WORKING_TREE, "typed into");
    press_identity_actions(&app, cx);
    assert_eq!(writes.get(), 0, "no press asks for a write");
    assert_eq!(
        identity_values(&app, cx),
        (typed.0.to_owned(), typed.1.to_owned()),
        "the typing as typed, Reset edits restoring nothing"
    );

    drop(held);
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    let email = if typed.1.is_empty() {
        "changed@example.invalid"
    } else {
        typed.1
    };
    assert_eq!(
        identity_values(&app, cx),
        (typed.0.to_owned(), email.to_owned()),
        "the typing stays, a field left empty takes what the read finds"
    );
    for line in [
        "settings-identity-line: Current identity: Fixture <changed@example.invalid>",
        "settings-signing-line: Commit signing is off in Git. Your existing Git signing \
         configuration is preserved.",
        IDENTITY_NOTES[0],
    ] {
        assert!(
            page_bounds(&app, cx, line).is_some(),
            "what the read finds: {line}"
        );
    }
    assert_eq!(writes.get(), 0, "nothing written meanwhile");
    save_identity_by_click(&app, cx).await;
    assert_eq!(writes.get(), 1, "Save asks for one identity write");
    assert_eq!(
        configured_identity(&root),
        (typed.0.to_owned(), email.to_owned()),
        "what the person typed, with what the read after the mark found"
    );
}

#[gpui::test]
async fn settings_identity_waits_for_the_read_after_a_refresh_clears_the_mark(
    cx: &mut TestAppContext,
) {
    settings_identity_waits_through_the_read_after_the_mark(cx, ("Typed", "")).await;
}

#[gpui::test]
async fn settings_identity_typed_as_before_the_mark_waits_for_the_read_after_it(
    cx: &mut TestAppContext,
) {
    let before = ("Fixture", "fixture@example.invalid");
    settings_identity_waits_through_the_read_after_the_mark(cx, before).await;
}

/// What Settings' fields held for another repository, its identity or a
/// Name typed for it (`typed`), never follows to the tab shown next: with
/// Settings shown on one tab and then the other shown, marked, and read
/// again once its `.git` is back, Settings draws that tab's own identity,
/// Save is not offered, and a click on it writes nothing. Settings' fields,
/// not shown since the other tab, still held the other repository's text as
/// the mark landed, which took it for the person's typing and kept it
/// through the mark, so Save wrote the other repository's identity into
/// this one (the security re-check of FU44-D10, X1 and X2).
async fn settings_identity_of_another_tab_never_follows_into_a_marked_one(
    cx: &mut TestAppContext,
    typed: bool,
) {
    use crate::workspace::refusals::{self, Refusal};
    let (fixture, app, cx, [root, _other]) = two_identities(cx).await;
    switch_tab(&app, cx, 1).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    if typed {
        type_into(cx, &fields[0], " Typed");
    }
    assert_eq!(
        identity_values(&app, cx),
        (
            if typed {
                "Other Name Typed"
            } else {
                "Other Name"
            }
            .to_owned(),
            "other@example.invalid".to_owned()
        ),
        "the other tab's identity"
    );
    switch_tab(&app, cx, 0).await;
    let away = fixture.path().join("git-away");
    mark_for_real(&app, cx, &root, &away).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    assert_eq!(
        identity_values(&app, cx),
        (String::new(), String::new()),
        "the marked tab draws nothing of the other"
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
    settle(&app, cx).await;
    std::fs::rename(&away, root.join(".git")).unwrap();
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
    settle(&app, cx).await;
    settle(&app, cx).await;
    assert!(
        !app.read_with(cx, |app, _| app.shown_tab_unavailable()),
        "the Refresh clears the mark"
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let own = ("Fixture".to_owned(), "fixture@example.invalid".to_owned());
    assert_eq!(identity_values(&app, cx), own, "the tab's own identity");
    page_bounds(&app, cx, SAVE_IDENTITY);
    assert_eq!(
        refusals::get(SAVE_IDENTITY),
        Some(Refusal {
            announced: true,
            dimmed: true,
        }),
        "Save not offered for what the read finds"
    );
    let (writes, _counting) = identity_writes(&app, cx);
    save_identity_by_click(&app, cx).await;
    assert_eq!(writes.get(), 0, "a click on Save asks for no write");
    assert_eq!(
        configured_identity(&root),
        own,
        "the repository's configuration is as it was"
    );
}

#[gpui::test]
async fn settings_identity_read_for_another_tab_never_follows_into_a_marked_one(
    cx: &mut TestAppContext,
) {
    settings_identity_of_another_tab_never_follows_into_a_marked_one(cx, false).await;
}

#[gpui::test]
async fn settings_identity_typed_for_another_tab_never_follows_into_a_marked_one(
    cx: &mut TestAppContext,
) {
    settings_identity_of_another_tab_never_follows_into_a_marked_one(cx, true).await;
}

/// A field the person typed in before the tab's mark brings back nothing
/// read before it, whichever page shows as the mark and the read after it
/// land: with a Name typed in Settings and left unsaved (`typed` 0; an Email
/// for 1), History shown, the tab marked there, its `.git` back with the
/// other value changed on disk and a Refresh landing in History, Settings
/// shows the typing and, in the other field, what that read found, and
/// Save writes those. The field the read had filled was emptied at the
/// mark only while Settings showed, so it kept the value read before the
/// mark, taken with the typing for one unfinished edit as the read landed,
/// and Save wrote it over the one configured since (the code re-check of
/// FU44-D11).
async fn settings_identity_typed_before_a_mark_behind_history(
    cx: &mut TestAppContext,
    typed: usize,
) {
    let (fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = app.read_with(cx, |app, _| {
        [app.identity_name_for_test(), app.identity_email_for_test()]
    });
    type_into(cx, &fields[typed], "x");
    let mut expected = ["Fixturex", "fixture@example.invalid"];
    if typed == 1 {
        expected = ["Fixture", "fixture@example.invalidx"];
    }
    assert_eq!(
        identity_values(&app, cx),
        (expected[0].to_owned(), expected[1].to_owned()),
        "the identity read, one field typed in"
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
    settle(&app, cx).await;

    // In History: the mark, the other field's value changed on disk, and
    // the Refresh whose reads land.
    let away = fixture.path().join("git-away");
    mark_for_real(&app, cx, &root, &away).await;
    std::fs::rename(&away, root.join(".git")).unwrap();
    let (key, changed) = [
        ("user.email", "changed@example.invalid"),
        ("user.name", "Changed Name"),
    ][typed];
    fixture_git(&root, &["config", key, changed]);
    expected[1 - typed] = changed;
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
    settle(&app, cx).await;
    settle(&app, cx).await;
    assert!(
        app.read_with(cx, |app, _| !app.shown_tab_unavailable()
            && app.page == AppPage::Repository),
        "the Refresh clears the mark, History still shown"
    );

    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let expected = (expected[0].to_owned(), expected[1].to_owned());
    assert_eq!(
        identity_values(&app, cx),
        expected,
        "the typing as typed, the other field what the read after the mark found"
    );
    let (writes, _counting) = identity_writes(&app, cx);
    save_identity_by_click(&app, cx).await;
    assert_eq!(writes.get(), 1, "Save asks for one identity write");
    assert_eq!(
        configured_identity(&root),
        expected,
        "the typing with what the read after the mark found"
    );
}

#[gpui::test]
async fn settings_identity_name_typed_before_a_mark_behind_history_keeps_no_email_from_before(
    cx: &mut TestAppContext,
) {
    settings_identity_typed_before_a_mark_behind_history(cx, 0).await;
}

#[gpui::test]
async fn settings_identity_email_typed_before_a_mark_behind_history_keeps_no_name_from_before(
    cx: &mut TestAppContext,
) {
    settings_identity_typed_before_a_mark_behind_history(cx, 1).await;
}

/// On a marked tab Settings' dimmed Name and Email leave the Tab order, as
/// the dimmed buttons after them do: a disabled field draws no focus ring,
/// so Tab stopped on them with nothing drawn to show where focus was
/// (decided 2026-11-14). They are Tab stops again as the mark clears.
#[gpui::test]
async fn settings_identity_fields_leave_the_tab_order_on_a_marked_tab(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let fields = app.read_with(cx, |app, cx| {
        [
            app.identity_name_for_test().focus_handle(cx),
            app.identity_email_for_test().focus_handle(cx),
        ]
    });
    // Whether Tab, pressed round the whole window from where focus is,
    // stops on Name and on Email.
    let stops = |cx: &mut VisualTestContext| {
        let mut round: Vec<FocusHandle> = Vec::new();
        for _ in 0..400 {
            cx.simulate_keystrokes("tab");
            draw(cx);
            let focused = cx
                .update(|window, cx| window.focused(cx))
                .expect("Tab keeps focus");
            if round.contains(&focused) {
                return fields.clone().map(|field| round.contains(&field));
            }
            round.push(focused);
        }
        panic!("Tab never came round");
    };
    assert_eq!(stops(cx), [true; 2], "Tab stops on both before the mark");
    mark(&app, cx, Some("Repository unavailable"));
    assert_eq!(stops(cx), [false; 2], "on neither on the marked tab");
    mark(&app, cx, None);
    assert_eq!(stops(cx), [true; 2], "on both as the mark clears");
}

/// Settings' Save repository identity and Reset edits on a marked tab are
/// refused with the mark's reason, as Manage named profiles… is: announced
/// unavailable and dimmed, the reason in their tooltips and accessible
/// names, and no route acts. Return in the Name or the Email field, a
/// single press, a held Return's repeats, marked held or fresh as X11 sends
/// them, and a release with its press, and the click start no write, so the
/// repository read before the mark keeps its `user.name`, which Save wrote
/// where its `.git` came back before a read (the security re-check of
/// FU44-D3), and Reset edits refills nothing. Offered again as the mark
/// clears, when the same clicks act: Reset edits refills the identity read
/// and Save saves once. Each is clicked where the page shows it, scrolled
/// to as a person scrolls (`wheeled_into_view`): clicked where the page
/// laid it out, below the window's foot, neither reached anything, mark or
/// no mark.
#[gpui::test]
async fn settings_identity_actions_say_the_mark(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::{REPEATS, enter};
    use crate::workspace::refusals::{self, Refusal};
    const SAVE: &str = "save-repository-identity";
    const RESET: &str = "use-current-identity";
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow_mut().clear());
    refusals::forget();
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let configured = || {
        let output = std::process::Command::new("git")
            .args(["config", "--local", "--get", "user.name"])
            .current_dir(&root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    let before = configured();
    // An edit of both, so Save and Reset edits are offered, and the mark,
    // which leaves typing as typed, leaves Save a whole identity to write
    // were a press not refused.
    let edit = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.identity_name_for_test()
                    .update(cx, |input, cx| input.set_value("Another Name", window, cx));
                app.identity_email_for_test().update(cx, |input, cx| {
                    input.set_value("another@example.invalid", window, cx)
                });
            })
        });
        draw(cx);
    };
    edit(cx);
    let offered = Some(Refusal {
        announced: false,
        dimmed: false,
    });
    for (key, label) in [(SAVE, "Save repository identity"), (RESET, "Reset edits")] {
        assert_eq!(
            refusals::get(key),
            offered,
            "{key}: offered before the mark"
        );
        assert_eq!(
            drawn_button(key),
            Some((Some(label.into()), false)),
            "{key}: drawn under its own name"
        );
    }
    set(&app, cx, |app| {
        let index = app.repository_tabs.active.expect("a tab");
        app.repository_tabs.tabs[index].error = Some("Repository unavailable".into());
    });
    for (key, tooltip, name) in [
        (
            SAVE,
            "Save repository identity · Repository unavailable",
            "Save repository identity: Repository unavailable",
        ),
        (
            RESET,
            "Restore the repository’s current name and email · Repository unavailable",
            "Reset edits: Repository unavailable",
        ),
    ] {
        assert_eq!(
            refusals::get(key),
            Some(Refusal {
                announced: true,
                dimmed: true,
            }),
            "{key}: refused and dimmed on the marked tab"
        );
        assert_eq!(
            crate::views::CONTROL_TOOLTIPS
                .with(|tooltips| tooltips.borrow().get(key).cloned())
                .as_deref(),
            Some(tooltip),
            "{key}: its tooltip says why"
        );
        assert_eq!(
            drawn_button(key),
            Some((Some(name.into()), true)),
            "{key}: drawn unavailable, its accessible name saying why"
        );
    }
    // Return in each field, then the click. Git's configuration is locked,
    // as another Git process can hold it, so a press the mark did not
    // refuse asks for a write that fails at once, leaving the identity as
    // it was: each press that asks for one is counted, whatever the press
    // before it asked.
    let lock = root.join(".git").join("config.lock");
    std::fs::write(&lock, "").unwrap();
    let writes = std::rc::Rc::new(std::cell::Cell::new(0_usize));
    let _counting = cx.update(|_, cx| {
        let writes = writes.clone();
        let mut was = false;
        cx.observe(&app, move |app, cx| {
            let now = app.read(cx).operation_busy == Some("Saving repository identity…");
            writes.set(writes.get() + usize::from(now && !was));
            was = now;
        })
    });
    let fields = app.read_with(cx, |app, _| {
        [
            (app.identity_name_for_test(), "Name"),
            (app.identity_email_for_test(), "Email"),
        ]
    });
    let mut asked = Vec::new();
    for (field, which) in &fields {
        cx.update(|window, cx| field.read(cx).focus_handle(cx).focus(window, cx));
        draw(cx);
        let presses = std::iter::once(("a single press", &[Some(false)][..]))
            .chain(REPEATS.iter().map(|(repeat, events)| (*repeat, *events)))
            .chain(std::iter::once(("its key-up", &[None][..])));
        for (press, events) in presses {
            let from = writes.get();
            for down in events {
                enter(cx, *down);
            }
            app.read_with(cx, |app, _| app.operations.drain());
            cx.run_until_parked();
            draw(cx);
            asked.push((format!("{which}, {press}"), writes.get() - from));
        }
    }
    assert_eq!(
        asked,
        asked
            .iter()
            .map(|(press, _)| (press.clone(), 0))
            .collect::<Vec<_>>(),
        "no Return in Name or Email asks for a write"
    );
    std::fs::remove_file(&lock).unwrap();
    // Each clicked where the page shows it, scrolled to it as a person
    // scrolls: at the window's usual size the card lies below its foot.
    let click = |cx: &mut VisualTestContext, key: &'static str| {
        let control = wheeled_into_view(&app, cx, key);
        cx.simulate_click(control.center(), gpui::Modifiers::default());
    };
    let name = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, cx| {
            app.identity_name_for_test().read(cx).value().to_string()
        })
    };
    click(cx, SAVE);
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    assert_eq!(writes.get(), 0, "the click on Save asks for no write");
    click(cx, RESET);
    settle(&app, cx).await;
    assert_eq!(
        configured(),
        before,
        "the repository's identity is as it was"
    );
    assert_eq!(name(cx), "Another Name", "Reset edits refills nothing");
    // The mark clears with a read, which a cleared mark's fields wait for.
    land_identity(&app, cx, ("Read Again", "again@example.invalid"), true);
    edit(cx);
    for (key, label) in [(SAVE, "Save repository identity"), (RESET, "Reset edits")] {
        assert_eq!(
            refusals::get(key),
            offered,
            "{key}: offered as the mark clears"
        );
        assert_eq!(
            drawn_button(key),
            Some((Some(label.into()), false)),
            "{key}: its own name again"
        );
    }
    // With no mark the same clicks act: Reset edits refills the identity
    // read, and Save, after another edit, saves once.
    click(cx, RESET);
    settle(&app, cx).await;
    assert_eq!(name(cx), "Read Again", "with no mark Reset edits refills");
    edit(cx);
    click(cx, SAVE);
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    assert_eq!(writes.get(), 1, "with no mark the click on Save saves once");
    assert_eq!(configured(), "Another Name", "what it saved");
}

/// A Save pressed before the tab's mark lands and released after it, over
/// the Save the marked tab draws, writes nothing: the press ends on a Save
/// drawn refused, so the repository read before the mark keeps its
/// identity. With no mark, the same press and release saves.
#[gpui::test]
async fn settings_identity_save_pressed_before_the_mark_writes_nothing(cx: &mut TestAppContext) {
    const SAVE: &str = "save-repository-identity";
    let (_fixture, app, cx) = history_window(cx).await;
    hold_quiet_reads_for_test(&app, cx);
    // Tall enough to show Save without scrolling the page, which a
    // pointer's press needs: in a window of the usual size it lies below
    // the window's foot, where no press reaches it.
    cx.simulate_resize(gpui::size(gpui::px(1480.), gpui::px(4400.)));
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
    let configured = || {
        ["user.name", "user.email"].map(|key| {
            let output = std::process::Command::new("git")
                .args(["config", "--local", "--get", key])
                .current_dir(&root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        })
    };
    let before = configured();
    // An unsaved edit of both, which the mark leaves as typed, so a press
    // the mark did not refuse would save it.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.identity_name_for_test()
                .update(cx, |input, cx| input.set_value("Pressed Name", window, cx));
            app.identity_email_for_test().update(cx, |input, cx| {
                input.set_value("pressed@example.invalid", window, cx)
            });
        })
    });
    draw(cx);
    let writes = std::rc::Rc::new(std::cell::Cell::new(0_usize));
    let _counting = cx.update(|_, cx| {
        let writes = writes.clone();
        let mut was = false;
        cx.observe(&app, move |app, cx| {
            let now = app.read(cx).operation_busy == Some("Saving repository identity…");
            writes.set(writes.get() + usize::from(now && !was));
            was = now;
        })
    });
    let center = page_bounds(&app, cx, SAVE).expect("Save").center();
    let viewport = cx.update(|window, _| window.viewport_size());
    assert!(
        center.y < viewport.height,
        "Save lies inside the window, at {center:?} in {viewport:?}"
    );
    let left = gpui::MouseButton::Left;
    cx.simulate_mouse_down(center, left, gpui::Modifiers::default());
    mark(&app, cx, Some("Repository unavailable"));
    // Released over the Save the marked tab draws, which the signing line
    // it leaves out moves up: released where it was pressed, the press
    // ended off it.
    let released = page_bounds(&app, cx, SAVE).expect("Save").center();
    cx.simulate_mouse_up(released, left, gpui::Modifiers::default());
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    assert_eq!(
        writes.get(),
        0,
        "the press ending on the marked tab asks for no write"
    );
    assert_eq!(
        configured(),
        before,
        "the repository's identity is as it was"
    );

    // The mark clears with a read, which Save waits for.
    land_identity(&app, cx, ("Read Again", "again@example.invalid"), true);
    let center = page_bounds(&app, cx, SAVE).expect("Save").center();
    cx.simulate_mouse_down(center, left, gpui::Modifiers::default());
    cx.simulate_mouse_up(center, left, gpui::Modifiers::default());
    app.read_with(cx, |app, _| app.operations.drain());
    settle(&app, cx).await;
    assert_eq!(writes.get(), 1, "with no mark the same press saves");
    assert_eq!(
        configured(),
        ["Pressed Name", "pressed@example.invalid"].map(str::to_owned),
        "what it saved"
    );
}

/// A New profile… form saved on a marked tab, where the manager the save
/// goes on to is refused (`open_profiles`), confirms the save where the
/// person is: "Profile preferences saved" in the window's notice, as a save
/// with no form showing says it. The form closed, the manager stayed
/// closed, and nothing said the profile had been saved.
#[gpui::test]
async fn a_profile_saved_on_a_marked_tab_is_confirmed_where_the_person_is(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    use_store(&app, cx, Store::default()).await;
    hold_quiet_reads_for_test(&app, cx);
    set(&app, cx, |app| {
        let index = app.repository_tabs.active.expect("a tab");
        app.repository_tabs.tabs[index].error = Some("Repository unavailable".into());
    });
    cx.update(|window, cx| app.update(cx, |app, cx| app.edit_profile(None, window, cx)));
    settle(&app, cx).await;
    let form = app
        .read_with(cx, |app, _| app.profiles.draft.clone())
        .expect("New profile… opens on a marked tab");
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    cx.update(|window, cx| {
        form.update(cx, |form, cx| {
            for (field, value) in [
                (&form.label, "Work"),
                (&form.name, "Profile Author"),
                (&form.email, "work@example.invalid"),
            ] {
                field.update(cx, |field, cx| field.set_value(value, window, cx));
            }
            form.submit(window, cx);
        })
    });
    let barrier = app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(())));
    barrier.await.unwrap().unwrap();
    settle(&app, cx).await;
    assert_eq!(
        app.read_with(cx, |app, _| app
            .profiles
            .store
            .as_ref()
            .map(|store| store.definitions.len())),
        Some(1),
        "saved"
    );
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "the form closes, and no manager opens on the marked tab"
    );
    assert_eq!(
        app.read_with(cx, |app, _| app.operation_notice.clone())
            .as_deref(),
        Some("Profile preferences saved"),
        "the save is confirmed"
    );
    assert!(
        cx.debug_bounds("operation-notice-text").is_some(),
        "where the person is"
    );
}

/// Two saved profiles, Work and Personal.
fn two_profiles() -> Store {
    let mut store = Store::default();
    store.definitions = vec![
        definition("work", "Work", "qa@work.example"),
        definition("personal", "Personal", "qa@home.example"),
    ];
    store
}

/// Changes the app's state with the menu open and draws, which builds the
/// open menu again where what it follows changed.
fn set(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, change: impl FnOnce(&mut GitTurtle)) {
    app.update(cx, |app, cx| {
        change(app);
        cx.notify();
    });
    draw(cx);
}

/// Each row of the open profile menu states its own reason after every
/// step, in each order a profile save and the reads its items wait for can
/// cross: a save that starts while the saved profiles are first read, the
/// read landing during it, or failing; and one that starts while the
/// repository is read, the read landing during it, or failing. The menu's
/// key once ranked the save over every read while its rows ranked a reason
/// of their own first, so a read that landed during a save left the open
/// menu without the saved profiles, and one that failed left New profile…
/// or the saved profiles stating the save, until the save ended.
#[gpui::test]
async fn the_open_profile_menu_follows_each_rows_reason_through_a_save(cx: &mut TestAppContext) {
    const WORK: &str = "Work · qa@work.example";
    const PERSONAL: &str = "Personal · qa@home.example";
    const CREATE: &str = "New profile…";
    const MANAGE: &str = "Manage profiles…";
    const RETRY: &str = "Retry profile storage";
    const SAVE: &str = "available when saving profile finishes";
    let (_fixture, app, cx) = history_window(cx).await;
    use_store(&app, cx, two_profiles()).await;
    hold_quiet_reads_for_test(&app, cx);
    let saving = |names: &[&str]| -> Vec<String> {
        names.iter().map(|name| format!("{name}, {SAVE}")).collect()
    };
    let rows_are = |cx: &mut VisualTestContext, expected: &[String], step: &str| {
        assert_eq!(rows(cx), expected, "{step}");
    };
    let row = |name: &str, why: &str| format!("{name}, {why}");

    // The saved profiles are first read as a save starts; the read lands
    // during the save.
    set(&app, cx, |app| app.profiles.store = None);
    open_menu(&app, cx).await;
    rows_are(
        cx,
        &[
            row(CREATE, "available when loading profiles finishes"),
            MANAGE.into(),
        ],
        "New profile… waits for the saved profiles' first read",
    );
    set(&app, cx, |app| app.profiles.saving = true);
    rows_are(cx, &saving(&[CREATE, MANAGE]), "the save refuses both");
    set(&app, cx, |app| app.profiles.store = Some(two_profiles()));
    rows_are(
        cx,
        &saving(&[WORK, PERSONAL, CREATE, MANAGE]),
        "the read lands during the save: the saved profiles are listed, waiting for it",
    );
    set(&app, cx, |app| app.profiles.saving = false);
    rows_are(
        cx,
        &[WORK.into(), PERSONAL.into(), CREATE.into(), MANAGE.into()],
        "the save's end offers every row",
    );

    // The same read fails during the save.
    set(&app, cx, |app| app.profiles.store = None);
    set(&app, cx, |app| app.profiles.saving = true);
    rows_are(cx, &saving(&[CREATE, MANAGE]), "the save refuses both");
    set(&app, cx, |app| {
        app.profiles.error = Some("store unreadable".into())
    });
    rows_are(
        cx,
        &[
            row(CREATE, "storage unavailable"),
            row(MANAGE, SAVE),
            RETRY.into(),
        ],
        "the read fails during the save: New profile… says why beside it",
    );
    set(&app, cx, |app| app.profiles.saving = false);
    rows_are(
        cx,
        &[
            row(CREATE, "storage unavailable"),
            MANAGE.into(),
            RETRY.into(),
        ],
        "the save's end offers Manage profiles…",
    );
    cx.simulate_keystrokes("escape");
    use_store(&app, cx, two_profiles()).await;

    // The repository is read as a save starts; the read lands during the
    // save.
    let repository = app.update(cx, |app, _| app.repository.take());
    set(&app, cx, |app| app.loading = Some("Reading local history…"));
    open_menu(&app, cx).await;
    let reading = "available when reading local history finishes";
    rows_are(
        cx,
        &[
            row(WORK, reading),
            row(PERSONAL, reading),
            CREATE.into(),
            MANAGE.into(),
        ],
        "a saved profile waits for the repository's read",
    );
    set(&app, cx, |app| app.profiles.saving = true);
    rows_are(
        cx,
        &saving(&[WORK, PERSONAL, CREATE, MANAGE]),
        "the save refuses every row",
    );
    let read = repository.clone();
    set(&app, cx, move |app| {
        app.repository = read;
        app.loading = None;
    });
    rows_are(
        cx,
        &saving(&[WORK, PERSONAL, CREATE, MANAGE]),
        "the read lands during the save, which still refuses every row",
    );
    set(&app, cx, |app| app.profiles.saving = false);
    rows_are(
        cx,
        &[WORK.into(), PERSONAL.into(), CREATE.into(), MANAGE.into()],
        "the save's end offers every row",
    );

    // The same read fails during the save.
    set(&app, cx, |app| {
        app.repository = None;
        app.loading = Some("Reading local history…");
    });
    set(&app, cx, |app| app.profiles.saving = true);
    rows_are(
        cx,
        &saving(&[WORK, PERSONAL, CREATE, MANAGE]),
        "the save refuses every row",
    );
    set(&app, cx, |app| app.loading = None);
    rows_are(
        cx,
        &[
            row(WORK, "repository unavailable"),
            row(PERSONAL, "repository unavailable"),
            row(CREATE, SAVE),
            row(MANAGE, SAVE),
        ],
        "the read fails during the save: each saved profile says why beside it",
    );
    set(&app, cx, |app| app.profiles.saving = false);
    rows_are(
        cx,
        &[
            row(WORK, "repository unavailable"),
            row(PERSONAL, "repository unavailable"),
            CREATE.into(),
            MANAGE.into(),
        ],
        "the save's end offers the rest",
    );
    set(&app, cx, move |app| app.repository = repository);
    rows_are(
        cx,
        &[WORK.into(), PERSONAL.into(), CREATE.into(), MANAGE.into()],
        "the repository read again, every row is offered",
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
}

/// With the saved profiles listed and the menu open, a read or save of the
/// profile store that fails shows Retry profile storage, with the line that
/// says what failed, at once, and one that then succeeds takes both away.
/// The menu's key read only why items were refused, which a failure leaves
/// as it was once the saved profiles are listed, so the open menu went
/// without Retry until it was opened again, and kept it, and the failure's
/// line, after a read that succeeded.
#[gpui::test]
async fn the_open_profile_menu_offers_retry_as_the_store_fails(cx: &mut TestAppContext) {
    const RETRY: &str = "Retry profile storage";
    const FAILED: &str = "menu-item: Profiles: store unreadable · Label";
    let (_fixture, app, cx) = history_window(cx).await;
    use_store(&app, cx, two_profiles()).await;
    hold_quiet_reads_for_test(&app, cx);
    open_menu(&app, cx).await;
    let offered: Vec<String> = [
        "Work · qa@work.example",
        "Personal · qa@home.example",
        "New profile…",
        "Manage profiles…",
    ]
    .map(String::from)
    .into();
    assert_eq!(rows(cx), offered, "at rest Retry is not offered");
    assert!(!shown(cx, FAILED));

    set(&app, cx, |app| {
        app.profiles.error = Some("store unreadable".into())
    });
    let mut retrying = offered.clone();
    retrying.push(RETRY.into());
    assert_eq!(rows(cx), retrying, "the failure offers Retry at once");
    assert!(shown(cx, FAILED), "with the line that says what failed");

    set(&app, cx, |app| app.profiles.error = None);
    assert_eq!(rows(cx), offered, "a read that succeeds takes Retry away");
    assert!(!shown(cx, FAILED), "and the failure's line");
    cx.simulate_keystrokes("escape");
    draw(cx);
}
