//! The header profile menu's rows as the kit announces them: the AccessKit
//! node each entry writes, which the kit names in its debug selector
//! (`menu-item: <label> · <role>`), and each label's drawn style.
use super::*;
use crate::native_accessibility::menu_tests::menu_label_style;
use crate::views::tests::{draw, history_window, hold_quiet_reads_for_test, settle};
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
