//! The profiles dialog's body and the profile form take what the dialog's
//! cap leaves them, scroll the rest with its cue, and reveal the control the
//! keyboard moves focus onto.
use super::menu_tests::use_store;
use super::*;
use crate::focus_reveal::tests::{Control, assert_tab_reveals, control, focus_filter};
use crate::tags::tests::{
    assert_scroll_cue, content_mask, draw, holds, rendered, set_interface_text_size,
};
use crate::views::tests::{history_window, settle};
use core::prelude::v1::test;

/// `count` saved profiles.
fn saved(count: usize) -> Store {
    let mut store = Store::default();
    store.definitions = (0..count)
        .map(|n| Definition {
            id: format!("p{n}"),
            label: format!("Profile {n:02}"),
            name: "GitTurtle QA".into(),
            email: format!("p{n:02}@example.invalid"),
            signing: None,
        })
        .collect();
    store
}

/// The profiles dialog's controls for `count` profiles, in tab order: New
/// profile…, then each profile's Apply…, Edit… and Delete….
fn controls(count: usize) -> Vec<Control> {
    let mut controls = vec![(ElementId::from("new-git-profile"), "new-git-profile")];
    for index in 0..count {
        for name in ["apply-profile", "edit-profile", "delete-profile"] {
            controls.push(control(name, index));
        }
    }
    controls
}

/// Opens the profiles dialog and returns its body.
fn open(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> FocusReveal {
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_profiles(window, cx)));
    draw(cx);
    app.read_with(cx, |app, _| app.profiles.manager.clone())
        .expect("the profiles dialog opens")
}

/// Eight saved profiles run past the profiles dialog's body at 13 and 18 pt,
/// in a 461 × 490 half tile and in a 1480 × 800 window, and its scrollbar's
/// thumb follows the body's offset in a gutter clear of every control's
/// ring. Laid out at full height in the dialog's scrolling body, the
/// profiles ran past the dialog's fold, or past the body's own 430 px, with
/// nothing to say more lay there.
#[gpui::test]
async fn the_profiles_dialog_cues_profiles_past_its_fold(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    // The dialog slides in on the real clock; its geometry is read still.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    use_store(&app, cx, saved(8)).await;
    let body = open(&app, cx);
    let room = cx.update(|_, cx| appearance::button_ring_room(cx));
    let rows: Vec<&'static str> = controls(8)
        .into_iter()
        .map(|(_, selector)| selector)
        .collect();
    for (width, height) in [(461., 490.), (1480., 800.)] {
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        assert_cued(
            cx,
            &app,
            body.scroll(),
            ("profile-manager-content", "dialog-ok: Done"),
            room,
            &rows,
        );
    }
}

/// At 13 and 18 pt the body `list`, which keeps `room` around its
/// controls and the ring room below them in a wrapper, ends above its
/// dialog's `ok`, and its scrollbar's thumb follows its offset in a gutter
/// clear of every control's ring in `rows` (`tags::tests::assert_scroll_cue`).
fn assert_cued(
    cx: &mut VisualTestContext,
    app: &Entity<GitTurtle>,
    scroll: &ScrollHandle,
    (list, ok): (&'static str, &'static str),
    room: Pixels,
    rows: &[&'static str],
) {
    for text in [13, 18] {
        set_interface_text_size(cx, app, text);
        let body = rendered(cx, list);
        let footer = rendered(cx, ok);
        assert!(
            body.bottom() - room <= footer.top() + px(0.01),
            "at {text} pt {list} {body:?} runs past {ok} {footer:?}"
        );
    }
    assert_scroll_cue(cx, app, scroll, list, room, rows);
}

/// In a 461 × 490 half tile, Tab from New profile… through every profile's
/// Apply…, Edit… and Delete… and Shift+Tab back scrolls each into the
/// dialog's body with its ring, whole inside every clip around it.
#[gpui::test]
async fn tab_reveals_every_profiles_dialog_control(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    // The dialog slides in on the real clock; its geometry is read still.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    use_store(&app, cx, saved(4)).await;
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    let body = open(&app, cx);
    let controls = controls(4);
    focus_filter(cx, &body, &controls[0]);
    assert_tab_reveals(cx, &body, "profile-manager-content", &controls);
}

/// The profile form runs past what the dialog's cap leaves it in a 461 × 490
/// half tile at 13 and 18 pt, and its scrollbar's thumb follows its offset
/// in a gutter clear of its button's ring; Tab from the profile name onto
/// Use current repository signing settings shows the button whole with its
/// ring, inside every clip around it. Laid out at full height in the
/// dialog's scrolling body, the form ran past the fold with no cue, and the
/// button took focus there unseen.
#[gpui::test]
async fn the_profile_form_cues_and_reveals_what_lies_past_its_fold(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    // The dialog slides in on the real clock; its geometry is read still.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    use_store(&app, cx, Store::default()).await;
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.edit_profile(None, window, cx)));
    settle(&app, cx).await;
    draw(cx);
    let form = app
        .read_with(cx, |app, _| app.profiles.draft.clone())
        .expect("the form opens");
    let body = form.read_with(cx, |form, _| form.body.clone());
    let room = cx.update(|_, cx| appearance::button_ring_room(cx));
    const SIGNER: &str = "profile-use-current-signer";
    assert_cued(
        cx,
        &app,
        body.scroll(),
        ("profile-form", "dialog-ok: Save profile"),
        room,
        &[SIGNER],
    );

    let name = form.read_with(cx, |form, cx| form.label.read(cx).focus_handle(cx));
    for text in [13, 18] {
        set_interface_text_size(cx, &app, text);
        cx.update(|window, cx| {
            body.scroll().set_offset(point(px(0.), px(0.)));
            name.focus(window, cx);
            window.refresh();
        });
        draw(cx);
        for _ in 0..8 {
            if cx.update(|window, _| body.focused(window)) == Some(ElementId::from(SIGNER)) {
                break;
            }
            cx.simulate_keystrokes("tab");
            draw(cx);
        }
        assert_eq!(
            cx.update(|window, _| body.focused(window)),
            Some(ElementId::from(SIGNER)),
            "at {text} pt Tab reaches {SIGNER}"
        );
        let mask = content_mask(cx, SIGNER);
        let ring = rendered(cx, SIGNER).dilate(room);
        assert!(
            holds(mask, ring),
            "at {text} pt {SIGNER} and its ring {ring:?} lie outside what the dialog shows, {mask:?}"
        );
    }
    set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
}
