//! The kit's popup menu entries as the app's menus build them: a label that
//! titles the menu, a disabled item, available items, a separator and a
//! closing caption, as the project pane, profile and review Options menus do.
//! GPUI's test platform never activates accessibility, so the kit names the
//! AccessKit node each entry writes in its debug selector.
use crate::*;
use core::prelude::v1::test;
use gpui_kit::component::{
    Theme,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use std::{cell::RefCell, rc::Rc};

/// The items chosen from the probe's menu, in order.
type Chosen = Rc<RefCell<Vec<&'static str>>>;

/// The probe's items chosen and its button's focus, which the keyboard
/// opens the menu from.
struct MenuProbe(Chosen, FocusHandle);

thread_local! {
    /// The last probe's button focus, for a test to move the keyboard there.
    static TRIGGER: RefCell<Option<FocusHandle>> = const { RefCell::new(None) };
}

impl Render for MenuProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.0.clone();
        div().size_full().p_4().child(
            Button::new("menu-probe")
                .track_focus(&self.1)
                .debug_selector(|| "menu-probe".into())
                .label("Open")
                .app_menu(move |menu, _, _| {
                    let item = |name: &'static str| {
                        let chosen = chosen.clone();
                        PopupMenuItem::new(name)
                            .on_click(move |_, _, _| chosen.borrow_mut().push(name))
                    };
                    menu.label("Saved profiles")
                        .item(item("Unavailable").disabled(true))
                        .item(item("First"))
                        .item(item("Second"))
                        .separator()
                        .label("Showing the original Git diff")
                }),
        )
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
}

/// `key` coming up, as it does between two presses a person makes: the kit
/// takes an Enter that goes down again with no key-up or other key between
/// for a repeat of the press, as X11 sends a held key's repeats, and acts on
/// none of them.
fn key_up(cx: &mut VisualTestContext, key: &str) {
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse(key).unwrap(),
    });
    draw(cx);
}

/// The probe's window with its menu open, and the items chosen from it.
fn open_menu(cx: &mut TestAppContext) -> (&mut VisualTestContext, Chosen) {
    cx.update(gpui_kit::init);
    let chosen = Chosen::default();
    let (_, cx) = cx.add_window_view({
        let chosen = chosen.clone();
        move |window, cx| {
            let probe = cx.new(|cx| {
                let focus = cx.focus_handle();
                TRIGGER.with(|trigger| *trigger.borrow_mut() = Some(focus.clone()));
                MenuProbe(chosen, focus)
            });
            Root::new(probe, window, cx)
        }
    });
    draw(cx);
    let trigger = cx.debug_bounds("menu-probe").expect("the menu's button");
    cx.simulate_click(trigger.center(), Modifiers::default());
    draw(cx);
    (cx, chosen)
}

fn shown(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(selector.leak())
}

/// The entry the probe menu highlights for the keyboard, by the label of the
/// item whose bounds hold the cursor's fill, or `None` when nothing is
/// highlighted.
fn highlighted(cx: &mut VisualTestContext) -> Option<&'static str> {
    let highlight = cx.update(|window, cx| {
        let fill = Background::from(Theme::global(cx).list_active);
        let scale = window.scale_factor();
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background == fill)
            .map(|quad| {
                let center = quad.bounds.center();
                point(px(center.x.as_f32() / scale), px(center.y.as_f32() / scale))
            })
            .collect::<Vec<_>>()
    });
    let entries = [
        ("Saved profiles", "menu-item: Saved profiles · Label"),
        ("Unavailable", "menu-item: Unavailable · MenuItem disabled"),
        ("First", "menu-item: First · MenuItem"),
        ("Second", "menu-item: Second · MenuItem"),
        (
            "Showing the original Git diff",
            "menu-item: Showing the original Git diff · Label",
        ),
    ];
    let mut found = None;
    for (name, selector) in entries {
        let bounds = cx.debug_bounds(selector).expect(selector);
        if highlight.iter().any(|center| bounds.contains(center)) {
            assert!(found.is_none(), "one highlight, on {found:?} and {name}");
            found = Some(name);
        }
    }
    found
}

/// Down and Up move only between the items that can act: the first Down in a
/// menu that opens with a label highlights the first available item, not the
/// label or the disabled item after it, and both keys wrap past the label and
/// the closing caption. Enter then acts on what Down reached.
#[gpui::test]
fn menu_keys_pass_over_labels_and_disabled_items(cx: &mut TestAppContext) {
    let (cx, chosen) = open_menu(cx);
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    draw(cx);
    assert_eq!(highlighted(cx), None, "nothing until a key is pressed");
    for (key, expected) in [
        ("down", "First"),
        ("down", "Second"),
        ("down", "First"),
        ("up", "Second"),
        ("up", "First"),
        ("up", "Second"),
        ("down", "First"),
    ] {
        cx.simulate_keystrokes(key);
        draw(cx);
        assert_eq!(highlighted(cx), Some(expected), "after {key}");
    }
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(*chosen.borrow(), ["First"]);
    assert!(
        cx.debug_bounds("menu-item: First · MenuItem").is_none(),
        "the menu closed"
    );
}

/// A menu the keyboard opens, with Enter on its button, starts with the
/// keyboard's cursor on its first item that can act, passing over the label
/// and the disabled item above it, so Enter acts there and Down moves on; the
/// same menu opened by a click highlights nothing. A keyboard's menu
/// highlighted nothing until the first Down.
#[gpui::test]
fn a_menu_the_keyboard_opens_starts_on_its_first_action(cx: &mut TestAppContext) {
    let (cx, chosen) = open_menu(cx);
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    draw(cx);
    assert_eq!(highlighted(cx), None, "a click's menu highlights nothing");
    cx.simulate_keystrokes("escape");
    draw(cx);
    let trigger = cx.debug_bounds("menu-probe").expect("the menu's button");
    assert!(
        cx.debug_bounds("menu-item: First · MenuItem").is_none(),
        "Escape closed the menu"
    );
    let focus = TRIGGER
        .with(|trigger| trigger.borrow().clone())
        .expect("the button's focus");
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(
        cx.debug_bounds("menu-item: First · MenuItem").is_some(),
        "Enter on {trigger:?} opened the menu"
    );
    assert_eq!(highlighted(cx), Some("First"), "on opening");
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(highlighted(cx), Some("Second"), "after Down");
    cx.simulate_keystrokes("up enter");
    draw(cx);
    assert_eq!(*chosen.borrow(), ["First"]);
}

/// Enter or Space held on a button opens its menu on its first item that
/// can act, and the key's repeats, which the platform marks held, run
/// nothing there; a fresh press runs the item. The menu confirmed on every
/// repeat, Enter through its own binding and Space through the popover's
/// it lies in, so a held key ran First as soon as the menu opened.
/// macOS and Wayland mark the repeats held, Wayland with an input method
/// such as fcitx5 too, as on a default Omarchy, since the vendored
/// `gpui-pre-linux` turns text input on only for a focused text field;
/// under X11 a held Enter arrives as fresh presses, which the menu tells
/// from new ones by the press the base kit records
/// (`enter_press_is_repeat`), so a fresh press comes after the key-up.
#[gpui::test]
fn a_held_key_that_opened_a_buttons_menu_runs_nothing(cx: &mut TestAppContext) {
    let (cx, chosen) = open_menu(cx);
    let focus = TRIGGER
        .with(|trigger| trigger.borrow().clone())
        .expect("the button's focus");
    for key in ["enter", "space"] {
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.update(|window, cx| window.focus(&focus, cx));
        draw(cx);
        cx.simulate_keystrokes(key);
        draw(cx);
        assert!(
            cx.debug_bounds("menu-item: First · MenuItem").is_some(),
            "{key} on the button opened the menu"
        );
        for _ in 0..3 {
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap().with_simulated_ime(),
                is_held: true,
                prefer_character_input: false,
            });
            draw(cx);
        }
        assert!(
            chosen.borrow().is_empty(),
            "{key} held runs nothing: {:?}",
            chosen.borrow()
        );
        assert!(
            cx.debug_bounds("menu-item: First · MenuItem").is_some(),
            "{key} held leaves the menu open"
        );
        key_up(cx, key);
        cx.simulate_keystrokes(key);
        draw(cx);
        assert_eq!(
            std::mem::take(&mut *chosen.borrow_mut()),
            ["First"],
            "a fresh {key} runs First"
        );
    }
}

/// Enter or Space that chose an item from a button's menu, still held once
/// the menu has closed and focus is back on the button, reopens nothing:
/// the button opens its menu only on a fresh press, as the menu confirms an
/// item only on one, and a fresh press opens it again. The popover's own
/// binding toggled it on every repeat, so the held key reopened the menu
/// once, a flicker in which no item ran.
#[gpui::test]
fn a_held_key_that_chose_from_a_buttons_menu_reopens_nothing(cx: &mut TestAppContext) {
    let (cx, chosen) = open_menu(cx);
    let focus = TRIGGER
        .with(|trigger| trigger.borrow().clone())
        .expect("the button's focus");
    let open =
        |cx: &mut VisualTestContext| cx.debug_bounds("menu-item: First · MenuItem").is_some();
    for key in ["enter", "space"] {
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.update(|window, cx| window.focus(&focus, cx));
        draw(cx);
        cx.simulate_keystrokes(key);
        draw(cx);
        assert!(open(cx), "{key} on the button opened the menu");
        key_up(cx, key);
        cx.simulate_keystrokes(key);
        draw(cx);
        assert_eq!(
            std::mem::take(&mut *chosen.borrow_mut()),
            ["First"],
            "a fresh {key} runs First"
        );
        assert!(!open(cx), "and closes the menu");
        assert!(
            cx.update(|window, _| focus.is_focused(window)),
            "the closed menu returns focus to its button"
        );
        for _ in 0..3 {
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap().with_simulated_ime(),
                is_held: true,
                prefer_character_input: false,
            });
            draw(cx);
            assert!(!open(cx), "{key} still held reopens nothing");
        }
        assert!(chosen.borrow().is_empty(), "{key} held runs nothing");
        key_up(cx, key);
        cx.simulate_keystrokes(key);
        draw(cx);
        assert!(open(cx), "a fresh {key} opens the menu again");
    }
}

/// A menu with an item that says why it is unavailable, built as the app
/// builds one ([`super::detailed_item`]), a disabled item that says nothing
/// and an item that can act, with the button's focus that opens it.
struct ReasonProbe(Chosen, FocusHandle);

impl Render for ReasonProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.0.clone();
        div().size_full().p_4().child(
            Button::new("reason-probe")
                .track_focus(&self.1)
                .label("Open")
                .app_menu(move |menu, _, _| {
                    let item = |item: PopupMenuItem, name: &'static str| {
                        let chosen = chosen.clone();
                        item.on_click(move |_, _, _| chosen.borrow_mut().push(name))
                    };
                    menu.item(item(
                        super::detailed_item("Ignore…", "Untracked files only"),
                        "Ignore…",
                    ))
                    .item(item(
                        PopupMenuItem::new("Unavailable").disabled(true),
                        "Unavailable",
                    ))
                    .item(item(PopupMenuItem::new("First"), "First"))
                }),
        )
    }
}

/// The keys stop on an item that says why it is unavailable and pass over a
/// disabled item that says nothing. Opened from the keyboard, the menu
/// starts on First, its first item that can act; Down wraps to Ignore…,
/// named "Ignore…, untracked files only", Enter there chooses nothing and
/// leaves the menu open, and Down, Up, Home and End move between Ignore…
/// and First and never reach Unavailable.
#[gpui::test]
fn menu_keys_stop_on_a_stated_reason_and_pass_over_a_silent_one(cx: &mut TestAppContext) {
    const IGNORE: &str = "Ignore…, untracked files only";
    const ITEMS: [&str; 3] = [IGNORE, "Unavailable", "First"];
    cx.update(gpui_kit::init);
    let chosen = Chosen::default();
    let focus = Rc::new(RefCell::new(None));
    let (_, cx) = cx.add_window_view({
        let (chosen, focus) = (chosen.clone(), focus.clone());
        move |window, cx| {
            let probe = cx.new(|cx| {
                let handle = cx.focus_handle();
                *focus.borrow_mut() = Some(handle.clone());
                ReasonProbe(chosen, handle)
            });
            Root::new(probe, window, cx)
        }
    });
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    draw(cx);
    let trigger = focus.borrow().clone().expect("the button's focus");
    cx.update(|window, cx| window.focus(&trigger, cx));
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(cursor_among(cx, &ITEMS), ["First"], "on opening");
    for (key, on) in [
        ("down", IGNORE),
        ("enter", IGNORE),
        ("down", "First"),
        ("up", IGNORE),
        ("up", "First"),
        ("home", IGNORE),
        ("end", "First"),
    ] {
        cx.simulate_keystrokes(key);
        draw(cx);
        assert_eq!(cursor_among(cx, &ITEMS), [on], "after {key}");
    }
    assert!(
        cx.debug_bounds(format!("menu-item: {IGNORE} · MenuItem disabled").leak())
            .is_some(),
        "Ignore… is announced unavailable with its reason"
    );
    assert!(chosen.borrow().is_empty(), "Enter on Ignore… chose nothing");
}

/// Enter acts only on an entry that can act. The pointer's hover selects any
/// row, so a pointer resting on the disabled item, the label or the closing
/// caption leaves the selection there; Enter then does nothing and the menu
/// stays open. Enter ran the disabled item's handler. On an available item
/// the hover and Enter act as before.
#[gpui::test]
fn enter_under_the_pointer_acts_only_on_an_item_that_can_act(cx: &mut TestAppContext) {
    let (cx, chosen) = open_menu(cx);
    for selector in [
        "menu-item: Unavailable · MenuItem disabled",
        "menu-item: Saved profiles · Label",
        "menu-item: Showing the original Git diff · Label",
    ] {
        let row = cx.debug_bounds(selector).expect(selector);
        cx.simulate_mouse_move(row.center(), None, Modifiers::default());
        draw(cx);
        cx.simulate_keystrokes("enter");
        draw(cx);
        key_up(cx, "enter");
        assert_eq!(
            *chosen.borrow(),
            Vec::<&str>::new(),
            "Enter over {selector}"
        );
        assert!(
            cx.debug_bounds(selector).is_some(),
            "the menu stays open after Enter over {selector}"
        );
    }
    let first = cx
        .debug_bounds("menu-item: First · MenuItem")
        .expect("the first item");
    cx.simulate_mouse_move(first.center(), None, Modifiers::default());
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(*chosen.borrow(), ["First"], "Enter over an available item");
}

/// The keyboard's cursor in a menu is marked as a list's selected row is:
/// the `selected` fill and a 2 px leading marker of `accent`, at 3:1 or more
/// against that fill and against the menu's raised surface, in every
/// palette. It was the hover fill alone, the cursor's only cue, 1.19:1 off
/// the menu in Midnight and 1.23:1 in Braden. The pointer's hover keeps that
/// quieter fill and no marker, and takes the highlight from the keyboard.
#[gpui::test]
fn the_keyboard_cursor_in_a_menu_reads_three_to_one_in_every_palette(cx: &mut TestAppContext) {
    let (cx, _) = open_menu(cx);
    cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
    draw(cx);
    cx.simulate_keystrokes("down");
    draw(cx);
    let item = |cx: &mut VisualTestContext, name: &str| {
        shown(cx, format!("menu-item: {name} · MenuItem")).expect(name)
    };
    let mut failures = Vec::new();
    for (name, palette) in appearance::test_palettes() {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        draw(cx);
        let first = item(cx, "First");
        failures.extend(appearance::cursor_issues(cx, first, palette, &name));
    }

    let second = item(cx, "Second");
    cx.simulate_mouse_move(second.center(), None, Modifiers::default());
    for (name, palette) in appearance::test_palettes() {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        draw(cx);
        let (first, second) = (item(cx, "First"), item(cx, "Second"));
        let hover = Background::from(Hsla::from(rgb(palette.hover)));
        let (fills, markers) = appearance::painted_row(cx, second);
        if fills != [hover] || !markers.is_empty() {
            failures.push(format!("{name}: hovered {fills:?} {markers:?}"));
        }
        let (fills, markers) = appearance::painted_row(cx, first);
        if !fills.is_empty() || !markers.is_empty() {
            failures.push(format!("{name}: the keyboard's row {fills:?} {markers:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A label is text, not a fourth command: it is announced as a label, never
/// disabled, and draws in secondary text, `muted_foreground`, rather than
/// the disabled colour, in every palette. A disabled item is announced
/// disabled and an available one as a plain menu item.
#[gpui::test]
fn menu_labels_read_as_secondary_text_and_disabled_items_as_disabled(cx: &mut TestAppContext) {
    let (cx, _) = open_menu(cx);
    for entry in [
        "menu-item: Saved profiles · Label",
        "menu-item: Showing the original Git diff · Label",
        "menu-item: Unavailable · MenuItem disabled",
        "menu-item: First · MenuItem",
        "menu-item: Second · MenuItem",
    ] {
        assert!(shown(cx, entry.into()).is_some(), "{entry}");
    }
    for entry in [
        "menu-item: Saved profiles · MenuItem disabled",
        "menu-item: Saved profiles · Label disabled",
        "menu-item: Showing the original Git diff · MenuItem disabled",
    ] {
        assert!(shown(cx, entry.into()).is_none(), "not {entry}");
    }

    let mut failures = Vec::new();
    for (name, palette) in appearance::test_palettes() {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        draw(cx);
        let (muted, disabled) = cx.update(|_, cx| {
            let theme = Theme::global(cx);
            (theme.muted_foreground, theme.disabled_foreground())
        });
        assert_eq!(muted, Hsla::from(rgb(palette.muted)), "{name}");
        assert_ne!(muted, disabled, "{name}");
        for label in ["Saved profiles", "Showing the original Git diff"] {
            let text = |color: Hsla| format!("menu-label: {label} · {:?}", gpui::Rgba::from(color));
            if shown(cx, text(muted)).is_none() || shown(cx, text(disabled)).is_some() {
                failures.push(format!("{name}: {label:?} is not in muted_foreground"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The items among `items` whose row draws the keyboard's cursor, the
/// toolkit's `list_active` fill, available or refused, as the last frame
/// painted them.
pub(crate) fn cursor_among<'a>(cx: &mut VisualTestContext, items: &[&'a str]) -> Vec<&'a str> {
    let fill = cx.update(|_, cx| Background::from(Theme::global(cx).list_active));
    items
        .iter()
        .copied()
        .filter(|item| {
            ["MenuItem", "MenuItem disabled"].iter().any(|role| {
                cx.debug_bounds(format!("menu-item: {item} · {role}").leak())
                    .is_some_and(|bounds| {
                        crate::appearance::painted_row(cx, bounds).0.contains(&fill)
                    })
            })
        })
        .collect()
}

/// The style a menu label draws in, which the kit names for its row:
/// `menu-label-style: <label> · <size> px · weight <weight>`.
pub(crate) fn menu_label_style(label: &str, size: Pixels, weight: FontWeight) -> String {
    format!(
        "menu-label-style: {label} · {:.2} px · weight {}",
        f32::from(size),
        weight.0
    )
}

/// A label is a heading, not one more item: it takes the section labels' 11
/// px medium, as the navigation's LOCAL BRANCHES does, scaled with the
/// interface text, where an item draws the kit's `text_sm` (0.875 of the root
/// size, 11.375 px at 13 pt) at the window's regular weight. Colour alone
/// cannot set it apart, since secondary text is nearly as strong as text in
/// One Dark and Solarized Dark, so in every palette weight does: the label
/// is medium where the items are regular, and no larger than an item, so a
/// heading does not outweigh the commands it titles, as a 12 px one did.
#[gpui::test]
fn menu_labels_take_the_section_label_size_and_differ_from_items_by_weight(
    cx: &mut TestAppContext,
) {
    let (cx, _) = open_menu(cx);
    let labels = ["Saved profiles", "Showing the original Git diff"];
    // At 13 pt the section labels' 11 px, and at 16 pt that scaled; the
    // items' `text_sm` and the window's own weight, which items draw in.
    let sizes = |cx: &mut VisualTestContext, label: f32| {
        cx.update(|window, _| {
            (
                px(label),
                window.rem_size() * 0.875,
                window.text_style().font_weight,
            )
        })
    };
    for (text_size, label) in [(13, 11.), (16, 11. * 16. / 13.)] {
        cx.update(|window, cx| appearance::apply_text_sizes(text_size, 12, window, cx));
        let mut failures = Vec::new();
        for (name, palette) in appearance::test_palettes() {
            cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
            draw(cx);
            let (label, item, item_weight) = sizes(cx, label);
            assert!(label <= item, "{text_size} pt {name}: {label:?} > {item:?}");
            assert_ne!(
                FontWeight::MEDIUM,
                item_weight,
                "{text_size} pt {name}: items are regular"
            );
            for text in labels {
                let style = menu_label_style(text, label, FontWeight::MEDIUM);
                if shown(cx, style.clone()).is_none() {
                    failures.push(format!("{text_size} pt {name}: no {style}"));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}

/// A dropdown's trigger keeps its place among its siblings. The kit lays the
/// trigger out in a box of its own, which its siblings see, and that box takes
/// the trigger's margins and flex-item properties: upstream left them on the
/// trigger inside the box, where flex properties do nothing, so a trigger
/// meant to fill a row kept its content width. The margin applies once.
#[gpui::test]
fn a_dropdown_trigger_keeps_its_place_among_its_siblings(cx: &mut TestAppContext) {
    struct Row;
    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .debug_selector(|| "row".into())
                .flex()
                .w(px(300.))
                .child(
                    Button::new("grow")
                        .debug_selector(|| "grow".into())
                        .label("Grow")
                        .flex_1()
                        .mr_2()
                        .dropdown_menu(|menu, _, _| menu.item(PopupMenuItem::new("Act"))),
                )
                .child(div().debug_selector(|| "after".into()).w(px(50.)).h_4())
        }
    }
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|_, _| Row);
    draw(cx);
    let row = cx.debug_bounds("row").expect("the row");
    let grow = cx.debug_bounds("grow").expect("the trigger");
    let after = cx.debug_bounds("after").expect("its sibling");
    let margin = cx.update(|window, _| window.rem_size() * 0.5);
    assert_eq!(grow.left(), row.left(), "{grow:?} in {row:?}");
    assert_eq!(after.right(), row.right(), "the trigger fills the row");
    assert_eq!(
        after.left(),
        grow.right() + margin,
        "one margin between {grow:?} and {after:?}"
    );
}

/// The menus a row's right-click built, as weak handles.
type Built = Rc<RefCell<Vec<WeakEntity<PopupMenu>>>>;

/// A row with the app's right-click menu, drawn while `shown` holds, as a
/// History or Working Changes row is until a list change or a scroll takes
/// it away.
struct ContextRowProbe {
    shown: bool,
    built: Built,
}

impl Render for ContextRowProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let built = self.built.clone();
        div().size_full().p_4().when(self.shown, |this| {
            this.child(
                div()
                    .id("context-row")
                    .debug_selector(|| "context-row".into())
                    .h(px(24.))
                    .w_full()
                    .child("A row")
                    .app_context_menu(move |menu, _, cx| {
                        built.borrow_mut().push(cx.entity().downgrade());
                        menu.item(PopupMenuItem::new("Act"))
                    }),
            )
        })
    }
}

/// A right-click menu goes with its row. The kit's context menu keeps the
/// menu it opened, and the subscription to that menu's dismissal, in state
/// its element shares with the dismissal's callback; the callback held that
/// state strongly, a cycle through the app's subscriber set that kept the
/// menu, and everything its builder captured, for the rest of the app's
/// life once the row was gone.
#[gpui::test]
fn a_context_menu_goes_with_its_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let built = Built::default();
    let probe = Rc::new(RefCell::new(None));
    let (_, cx) = cx.add_window_view({
        let (built, probe) = (built.clone(), probe.clone());
        move |window, cx| {
            let row = cx.new(|_| ContextRowProbe { shown: true, built });
            *probe.borrow_mut() = Some(row.clone());
            Root::new(row, window, cx)
        }
    });
    let probe: Entity<ContextRowProbe> = probe.take().expect("the probe");
    draw(cx);
    let row = cx.debug_bounds("context-row").expect("the row");
    cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("menu-item: Act · MenuItem").is_some(),
        "the right-click opened the row's menu"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(
        cx.debug_bounds("menu-item: Act · MenuItem").is_none(),
        "Escape closed the menu"
    );

    probe.update(cx, |probe, cx| {
        probe.shown = false;
        cx.notify();
    });
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("context-row").is_none(), "the row is gone");
    let built = built.borrow();
    assert_eq!(built.len(), 1, "one menu was built");
    assert!(
        built[0].upgrade().is_none(),
        "the row's menu outlived the row"
    );
}

/// A dropdown and a row with a right-click menu at the foot of the window,
/// each opening the same four items with the kit's own menus.
struct LowMenusProbe;

impl Render for LowMenusProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::menu::ContextMenuExt as _;
        fn items(menu: PopupMenu) -> PopupMenu {
            ["One", "Two", "Three", "Four"]
                .into_iter()
                .fold(menu, |menu, name| menu.item(PopupMenuItem::new(name)))
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .gap_2()
            .p_2()
            .child(
                Button::new("low-dropdown")
                    .debug_selector(|| "low-dropdown".into())
                    .label("Open")
                    .dropdown_menu(|menu, _, _| items(menu)),
            )
            .child(
                div()
                    .id("low-row")
                    .debug_selector(|| "low-row".into())
                    .h(px(24.))
                    .w_full()
                    .child("A row")
                    .context_menu(|menu, _, _| items(menu)),
            )
    }
}

/// The open menu's surface: the last fill of the popover colour painted
/// around its first item.
fn open_menu_surface(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    menu_surface_around(cx, "menu-item: One · MenuItem")
}

/// The surface of the open menu that holds the item `selector` names: the
/// last fill of the popover colour painted around that item.
fn menu_surface_around(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    let item = cx.debug_bounds(selector).expect(selector);
    cx.update(|window, cx| {
        let fill = Background::from(Theme::global(cx).popover);
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
            .filter(|(_, surface)| surface.contains(&item.center()))
            .max_by_key(|(order, _)| *order)
            .map(|(_, surface)| surface)
            .expect("the menu's surface")
    })
}

/// A kit dropdown and a kit right-click menu opened at the foot of the
/// window stay above the room an application keeps there
/// (`gpui_kit::base::set_popup_bottom_inset`), whole pixels included, as
/// they stay inside the window's margin: they move up rather than run into
/// it. The dropdown's popup host kept only the window's 8 px margin and the
/// context menu's anchor moved it only once it ran past the window, so
/// both ended over a status bar.
#[gpui::test]
fn kit_menus_stay_above_the_bottom_inset_an_application_sets(cx: &mut TestAppContext) {
    const INSET: Pixels = px(120.5);
    cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::base::set_popup_bottom_inset(cx, |_| INSET);
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let probe = cx.new(|_| LowMenusProbe);
        Root::new(probe, window, cx)
    });
    cx.simulate_resize(size(px(400.), px(320.)));
    draw(cx);
    let foot = px(320.) - INSET;

    let trigger = cx.debug_bounds("low-dropdown").expect("the dropdown");
    cx.simulate_click(trigger.center(), Modifiers::default());
    draw(cx);
    draw(cx);
    let surface = open_menu_surface(cx);
    assert!(
        surface.bottom() <= foot,
        "the dropdown's menu {surface:?} ends above the inset at {foot:?}"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);

    let row = cx.debug_bounds("low-row").expect("the row");
    cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
    draw(cx);
    draw(cx);
    let surface = open_menu_surface(cx);
    assert!(
        surface.bottom() <= foot,
        "the row's menu {surface:?} ends above the inset at {foot:?}"
    );
    assert!(
        surface.top() < row.center().y,
        "the row's menu {surface:?} moved up from the pointer at {:?}",
        row.center()
    );
}

/// A row whose right-click menu's item opens a dialog, beside a field that
/// holds focus as the row is pressed.
struct PointerDialogProbe(FocusHandle);

impl Render for PointerDialogProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                div()
                    .id("pointer-field")
                    .h(px(24.))
                    .w_full()
                    .track_focus(&self.0),
            )
            .child(
                div()
                    .id("pointer-row")
                    .debug_selector(|| "pointer-row".into())
                    .h(px(24.))
                    .w_full()
                    .child("A row")
                    .app_context_menu(|menu, _, _| {
                        menu.item(PopupMenuItem::new("Open").on_click(|_, window, cx| {
                            window.open_dialog(cx, |dialog, _, _| {
                                dialog.child(
                                    div()
                                        .debug_selector(|| "pointer-dialog".into())
                                        .size(px(20.)),
                                )
                            });
                        }))
                    }),
            )
            .children(Root::render_dialog_layer(window, cx))
    }
}

/// A dialog an item of a row's right-click menu opens gives focus back, on
/// Escape, where the menu itself would have returned it: the app's own
/// right-click menu tells the kit that place, since it keeps it itself. The
/// dialog recorded the menu's focus, and the menu went as the item finished.
#[gpui::test]
fn a_dialog_from_a_right_click_menu_returns_focus_where_the_menu_would(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let field = Rc::new(RefCell::new(None));
    let (_, cx) = cx.add_window_view({
        let field = field.clone();
        move |window, cx| {
            let handle = cx.focus_handle();
            *field.borrow_mut() = Some(handle.clone());
            let probe = cx.new(|_| PointerDialogProbe(handle));
            Root::new(probe, window, cx)
        }
    });
    draw(cx);
    let field = field.borrow().clone().expect("the field");
    cx.update(|window, cx| window.focus(&field, cx));
    draw(cx);
    let row = cx.debug_bounds("pointer-row").expect("the row");
    cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
    draw(cx);
    draw(cx);
    let item = cx
        .debug_bounds("menu-item: Open · MenuItem")
        .expect("the menu's item");
    cx.simulate_click(item.center(), Modifiers::default());
    draw(cx);
    assert!(
        cx.debug_bounds("pointer-dialog").is_some(),
        "the item opened its dialog"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(
        cx.debug_bounds("pointer-dialog").is_none(),
        "Escape closes the dialog"
    );
    assert!(
        cx.update(|window, _| field.is_focused(window)),
        "focus is back on the field"
    );
}

/// A kit dropdown's button centred 40 px below the window's top, which a
/// change of the window's width moves.
struct CentredMenuProbe;

impl Render for CentredMenuProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().justify_center().pt(px(40.)).child(
            Button::new("centred-dropdown")
                .debug_selector(|| "centred-dropdown".into())
                .label("Open")
                .dropdown_menu(|menu, _, _| {
                    ["One", "Two", "Three"]
                        .into_iter()
                        .fold(menu, |menu, name| menu.item(PopupMenuItem::new(name)))
                }),
        )
    }
}

/// A kit dropdown left open follows its button in the frame a width change
/// draws. Its popup host placed the menu at the corner its button had in
/// the frame before, so the menu stayed where the button had been until
/// some other change drew a frame: a Compare width step draws one frame,
/// and the frame it once asked for after it hid this. The host now reads
/// the corner its button's prepaint records in the same frame (a gpui-base
/// patch, `Positioner::corner_following`).
#[gpui::test]
fn an_open_kit_dropdown_follows_its_button_when_the_window_changes_width(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let probe = cx.new(|_| CentredMenuProbe);
        Root::new(probe, window, cx)
    });
    cx.simulate_resize(size(px(600.), px(400.)));
    draw(cx);
    let trigger = cx.debug_bounds("centred-dropdown").expect("the dropdown");
    cx.simulate_click(trigger.center(), Modifiers::default());
    draw(cx);
    draw(cx);
    let item = cx
        .debug_bounds("menu-item: One · MenuItem")
        .expect("the open menu's first item");
    let beside = item.origin - trigger.origin;
    for width in [500., 540., 440.] {
        // The frame the window's resize draws, and no other.
        cx.simulate_resize(size(px(width), px(400.)));
        let moved = cx.debug_bounds("centred-dropdown").expect("the dropdown");
        let followed = cx
            .debug_bounds("menu-item: One · MenuItem")
            .expect("the menu stays open");
        assert!(
            (moved.left() - trigger.left()).abs() >= px(20.),
            "{width} px moves the button from {trigger:?} to {moved:?}"
        );
        let at = followed.origin - moved.origin;
        assert!(
            (at.x - beside.x).abs() <= px(1.) && (at.y - beside.y).abs() <= px(1.),
            "{width} px: the menu's first item {followed:?} keeps its place {beside:?} from the button {moved:?}, not {at:?}"
        );
    }
}

/// The app's dropdown (`AppMenu`) on a button `lead` below the window's top,
/// opening sixteen items, the first "One", and recording what is chosen.
struct AppDropdownProbe {
    lead: Pixels,
    chosen: Chosen,
}

impl Render for AppDropdownProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.chosen.clone();
        div().size_full().pt(self.lead).px_2().child(
            Button::new("app-dropdown")
                .debug_selector(|| "app-dropdown".into())
                .label("Open")
                .app_menu(move |menu, _, _| {
                    (0..16).fold(menu, |menu, index| {
                        let name: &'static str = if index == 0 {
                            "One"
                        } else {
                            format!("Item {index}").leak()
                        };
                        let chosen = chosen.clone();
                        menu.item(
                            PopupMenuItem::new(name)
                                .on_click(move |_, _, _| chosen.borrow_mut().push(name)),
                        )
                    })
                }),
        )
    }
}

/// A dropdown whose room below, down to the status bar's margin, holds
/// fewer than its least rows never covers its own button: it opens above it
/// where the room above holds them, and otherwise on the roomier side with
/// as many whole rows as that side holds, and a second click on the button
/// lands on the button and closes the menu, choosing nothing. Moved up by
/// the status bar's inset, the menu once covered its button, which a second
/// click then missed for an item.
#[gpui::test]
fn a_dropdown_kept_above_the_status_bar_never_covers_its_button(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    // In a 461 × 490 window, a button whose foot lies 43 px above the
    // menu's foot, and in a 461 × 230 window one with 57 px above it and
    // 89 px below, where neither side holds the least rows.
    for (height, lead, above) in [(490., 380., true), (230., 70., false)] {
        let chosen = Chosen::default();
        let (_, cx) = cx.add_window_view({
            let chosen = chosen.clone();
            move |window, cx| {
                let probe = cx.new(|_| AppDropdownProbe {
                    lead: px(lead),
                    chosen,
                });
                Root::new(probe, window, cx)
            }
        });
        cx.simulate_resize(size(px(461.), px(height)));
        draw(cx);
        let at = format!("461 × {height}");
        let foot = cx.update(|window, _| super::menu_foot(window));
        let button = cx.debug_bounds("app-dropdown").expect("the dropdown");
        let (below, over) = (foot - button.bottom(), button.top());
        assert!(
            below < appearance::ui_size(super::MENU_FLOOR),
            "{at}: {below:?} below the button holds fewer than the least rows"
        );
        cx.simulate_click(button.center(), Modifiers::default());
        draw(cx);
        draw(cx);
        let surface = open_menu_surface(cx);
        assert!(
            surface.bottom() <= button.top() || surface.top() >= button.bottom(),
            "{at}: the menu {surface:?} covers its button {button:?}"
        );
        assert_eq!(
            surface.bottom() <= button.top(),
            above,
            "{at}: the menu {surface:?} opens {} its button {button:?}, with {over:?} above \
             and {below:?} below",
            if above { "above" } else { "below" }
        );
        assert!(
            surface.bottom() <= foot && surface.top() >= px(0.),
            "{at}: the menu {surface:?} stays in the window above {foot:?}"
        );
        cx.simulate_click(button.center(), Modifiers::default());
        draw(cx);
        assert!(
            cx.debug_bounds("menu-item: One · MenuItem").is_none(),
            "{at}: a second click on the button closes the menu"
        );
        assert!(
            chosen.borrow().is_empty(),
            "{at}: the second click chose {:?}",
            chosen.borrow()
        );
    }
}

/// A button's menu open above it when the window grows, as a tiling
/// compositor re-tiles it, never ends over the button: the resize closes it,
/// its rows having been capped for the side the old size gave it, and
/// opened again it takes the side the new size gives it. Its side was
/// chosen again in every frame while its rows kept their cap, so a window
/// grown from 490 to 600 px turned it below its button with the rows capped
/// for above, and the kit moved it up over the button.
#[gpui::test]
fn a_resized_window_closes_a_button_s_menu_rather_than_turn_it_over_the_button(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    let (_, cx) = cx.add_window_view(|window, cx| {
        let probe = cx.new(|_| AppDropdownProbe {
            lead: px(380.),
            chosen: Chosen::default(),
        });
        Root::new(probe, window, cx)
    });
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    let button = cx.debug_bounds("app-dropdown").expect("the dropdown");
    cx.simulate_click(button.center(), Modifiers::default());
    draw(cx);
    draw(cx);
    let surface = open_menu_surface(cx);
    assert!(
        surface.bottom() <= button.top(),
        "at 490 px the menu {surface:?} opens above its button {button:?}"
    );
    for height in [520., 560., 600., 640., 700.] {
        cx.simulate_resize(size(px(461.), px(height)));
        draw(cx);
        let button = cx.debug_bounds("app-dropdown").expect("the dropdown");
        if cx.debug_bounds("menu-item: One · MenuItem").is_some() {
            let surface = open_menu_surface(cx);
            assert!(
                surface.bottom() <= button.top() || surface.top() >= button.bottom(),
                "at {height} px the menu {surface:?} covers its button {button:?}"
            );
        }
        draw(cx);
        assert!(
            cx.debug_bounds("menu-item: One · MenuItem").is_none(),
            "at {height} px the resize closed the menu"
        );
    }
    let button = cx.debug_bounds("app-dropdown").expect("the dropdown");
    cx.simulate_click(button.center(), Modifiers::default());
    draw(cx);
    draw(cx);
    let surface = open_menu_surface(cx);
    assert!(
        surface.top() >= button.bottom(),
        "opened again at 700 px the menu {surface:?} takes the room below its button {button:?}"
    );
}

/// A row's menu, sized for the room where it opened, leaves its row when
/// the row moves where the menu fits neither below it nor wholly above it,
/// or is not drawn: clamped to the room, it lay over the row it acts on.
/// Moved where it still fits on either side it stays, and so does a menu
/// whose row has not moved, even fitting on neither side, until that row
/// moves.
#[test]
fn a_row_menu_leaves_a_row_moved_where_it_no_longer_fits() {
    use super::{RowAnchor, leaves_its_row};
    let room = Bounds::from_corners(point(px(0.), px(9.)), point(px(400.), px(300.)));
    let row = |top: f32| RowAnchor {
        x: px(10.),
        top: px(top),
        below: px(top + 44.),
        asked: px(top + 44.),
        trailing: None,
    };
    let (menu, opened) = (size(px(160.), px(150.)), row(100.));
    assert!(
        !leaves_its_row(opened, (row(100.), true), menu, room),
        "as it opened"
    );
    assert!(
        !leaves_its_row(opened, (row(56.), true), menu, room),
        "a row up"
    );
    assert!(
        leaves_its_row(opened, (row(144.), true), menu, room),
        "a row down, room for it neither below nor above"
    );
    assert!(
        !leaves_its_row(opened, (row(250.), true), menu, room),
        "above it"
    );
    assert!(
        leaves_its_row(opened, (row(100.), false), menu, room),
        "no longer drawn"
    );
    let tall = size(px(160.), px(280.));
    assert!(
        !leaves_its_row(opened, (row(100.), true), tall, room),
        "unmoved, fitting on neither side"
    );
    assert!(
        leaves_its_row(opened, (row(101.), true), tall, room),
        "moved, fitting on neither side"
    );
}

/// A row `lead` below the window's top with the app's right-click menu of
/// four items, as History's and Working Changes' rows open theirs.
struct GapRowProbe {
    lead: Pixels,
}

impl Render for GapRowProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().pt(self.lead).px_4().child(
            div()
                .id("gap-row")
                .debug_selector(|| "gap-row".into())
                .h(px(24.))
                .w_full()
                .child("A row")
                .app_context_menu(|menu, _, _| {
                    ["One", "Two", "Three", "Four"]
                        .into_iter()
                        .fold(menu, |menu, name| menu.item(PopupMenuItem::new(name)))
                }),
        )
    }
}

/// The page between a menu's drawn edge, the ring outside its fill, and the
/// button or row it opened from (`appearance::popup_gap`, 4 px at 13 pt),
/// with the half pixel the kit's popup host rounds its place by.
fn assert_one_gap(gap: Pixels, at: &str) {
    let expected = appearance::popup_gap();
    assert!(
        (gap - expected).abs() <= px(0.5),
        "{at}: {gap:?} between the menu's drawn edge and what it opened from, not {expected:?}"
    );
}

/// Every menu keeps one gap, 4 px at 13 pt and scaled with the text, between
/// its drawn edge and the button or row it opened from: a button's menu
/// below or above its button, and a row's right-click menu moved wholly
/// above its row at the window's foot. A dropdown kept the kit's quarter rem
/// to its fill, its edge 2 or 2.5 px from its button at 13 pt, and a row's
/// menu above its row drew its edge on the row's top.
#[gpui::test]
fn menus_keep_one_gap_from_their_button_or_row(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    let edge = gpui_kit::component::POPOVER_EDGE_WIDTH;
    for text in [13, 18] {
        // A button with the room below it, and one low in the window, whose
        // menu opens above it.
        for (lead, above) in [(40., false), (380., true)] {
            let at = format!("{text} pt, a button {lead} px down");
            let (_, cx) = cx.add_window_view(move |window, cx| {
                let probe = cx.new(|_| AppDropdownProbe {
                    lead: px(lead),
                    chosen: Chosen::default(),
                });
                Root::new(probe, window, cx)
            });
            cx.update(|window, cx| {
                appearance::ThemeChoice::Midnight.apply(Some(window), cx);
                appearance::apply_text_sizes(text, 12, window, cx);
            });
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            let button = cx.debug_bounds("app-dropdown").expect("the dropdown");
            cx.simulate_click(button.center(), Modifiers::default());
            draw(cx);
            draw(cx);
            let drawn = open_menu_surface(cx).dilate(edge);
            if above {
                assert_one_gap(button.top() - drawn.bottom(), &format!("{at}, above it"));
            } else {
                assert_one_gap(drawn.top() - button.bottom(), &format!("{at}, below it"));
            }
            cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        }

        let at = format!("{text} pt, a row at the window's foot");
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| GapRowProbe { lead: px(400.) });
            Root::new(probe, window, cx)
        });
        cx.update(|window, cx| {
            appearance::ThemeChoice::Midnight.apply(Some(window), cx);
            appearance::apply_text_sizes(text, 12, window, cx);
        });
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let row = cx.debug_bounds("gap-row").expect("the row");
        cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
        draw(cx);
        draw(cx);
        let drawn = open_menu_surface(cx).dilate(edge);
        assert!(
            drawn.bottom() <= row.top(),
            "{at}: the menu {drawn:?} lies wholly above the row {row:?}"
        );
        assert_one_gap(row.top() - drawn.bottom(), &at);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }
}

/// The content mask the open menu's surface, read as [`open_menu_surface`]
/// reads it, is drawn in, in logical pixels: the clip its shadow is painted
/// in too.
fn open_menu_mask(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let item = cx
        .debug_bounds("menu-item: One · MenuItem")
        .expect("the menu's first item");
    cx.update(|window, cx| {
        let fill = Background::from(Theme::global(cx).popover);
        let scale = window.scale_factor();
        let logical = |bounds: Bounds<gpui_kit::ScaledPixels>| {
            Bounds::from_corners(
                point(
                    px(bounds.left().as_f32() / scale),
                    px(bounds.top().as_f32() / scale),
                ),
                point(
                    px(bounds.right().as_f32() / scale),
                    px(bounds.bottom().as_f32() / scale),
                ),
            )
        };
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background == fill)
            .filter(|quad| logical(quad.bounds).contains(&item.center()))
            .max_by_key(|quad| quad.order)
            .map(|quad| logical(quad.content_mask.bounds))
            .expect("the menu's surface")
    })
}

/// Nothing a menu draws reaches the button or row it opened from, its soft
/// shadow included, which falls 4 px with a 3 px blur, further than the
/// gap: a button's menu below its button begins where the button ends, one
/// above it ends where the button begins, and a row's right-click menu
/// moved wholly above its row ends where the row begins, each with its
/// fill and drawn edge whole. A menu above its button or row shaded it.
#[gpui::test]
fn menus_draw_nothing_over_their_button_or_row(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    let edge = gpui_kit::component::POPOVER_EDGE_WIDTH;
    for (lead, above) in [(40., false), (380., true)] {
        let at = format!("a button {lead} px down");
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let probe = cx.new(|_| AppDropdownProbe {
                lead: px(lead),
                chosen: Chosen::default(),
            });
            Root::new(probe, window, cx)
        });
        cx.update(|window, cx| appearance::ThemeChoice::Daylight.apply(Some(window), cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let button = cx.debug_bounds("app-dropdown").expect("the dropdown");
        cx.simulate_click(button.center(), Modifiers::default());
        draw(cx);
        draw(cx);
        let drawn = open_menu_surface(cx).dilate(edge);
        let mask = open_menu_mask(cx);
        if above {
            assert!(
                mask.bottom() <= button.top() + px(0.01)
                    && mask.bottom() >= drawn.bottom() - px(0.01),
                "{at}: the menu {drawn:?} above its button {button:?} draws within {mask:?}"
            );
        } else {
            assert!(
                mask.top() >= button.bottom() - px(0.01) && mask.top() <= drawn.top() + px(0.01),
                "{at}: the menu {drawn:?} below its button {button:?} draws within {mask:?}"
            );
        }
    }

    let (_, cx) = cx.add_window_view(|window, cx| {
        let probe = cx.new(|_| GapRowProbe { lead: px(400.) });
        Root::new(probe, window, cx)
    });
    cx.update(|window, cx| appearance::ThemeChoice::Daylight.apply(Some(window), cx));
    cx.simulate_resize(size(px(461.), px(490.)));
    draw(cx);
    let row = cx.debug_bounds("gap-row").expect("the row");
    cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
    draw(cx);
    draw(cx);
    let drawn = open_menu_surface(cx).dilate(edge);
    let mask = open_menu_mask(cx);
    assert!(
        drawn.bottom() <= row.top()
            && mask.bottom() <= row.top() + px(0.01)
            && mask.bottom() >= drawn.bottom() - px(0.01),
        "the menu {drawn:?} above its row {row:?} draws within {mask:?}"
    );
}

/// A long item, wider than a narrow window holds beside another menu.
const LONG_ITEM: &str = "An item whose text runs well past the side of a narrow window";

/// The app's dropdown (`AppMenu`) on a button at each side of the window,
/// a row across the window with the app's right-click menu, and the kit's
/// dropdown on a button at each side, each with a submenu, More, of
/// [`LONG_ITEM`], as no app menu has yet.
struct SideMenusProbe;

impl Render for SideMenusProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        fn items(menu: PopupMenu) -> PopupMenu {
            ["One", "Two", "Three", "Four"]
                .into_iter()
                .fold(menu, |menu, name| menu.item(PopupMenuItem::new(name)))
        }
        fn nested(menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
            items(menu).submenu("More", window, cx, |menu, _, _| {
                menu.item(PopupMenuItem::new(LONG_ITEM))
            })
        }
        let side = |name: &'static str| {
            Button::new(name)
                .debug_selector(move || name.into())
                .label("Open")
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p(px(2.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(side("left-dropdown").app_menu(|menu, _, _| items(menu)))
                    .child(side("right-dropdown").app_menu(|menu, _, _| items(menu))),
            )
            .child(
                div()
                    .id("side-row")
                    .debug_selector(|| "side-row".into())
                    .h(px(24.))
                    .w_full()
                    .child("A row")
                    .app_context_menu(|menu, _, _| items(menu)),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(side("left-nested").dropdown_menu(nested))
                    .child(side("right-nested").dropdown_menu(nested)),
            )
            // Buttons half a pixel further in, 2.5 px from either side, as
            // the Main Menu's button starts part way into a pixel.
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px(px(0.5))
                    .child(side("left-half-dropdown").app_menu(|menu, _, _| items(menu)))
                    .child(side("right-half-dropdown").app_menu(|menu, _, _| items(menu))),
            )
    }
}

/// How far `drawn`, a menu's drawn edge, stands from the window's side it
/// lies against, `left` or the right one of a `width` wide window: 8 px, on
/// the device pixel. The popup hosts moved a menu by whole pixels from where
/// they were laid out, so beside a button half a pixel into one, at this
/// window's scale of 2, its edge stood 8.5 px in.
fn assert_side_margin(drawn: Bounds<Pixels>, left: bool, width: Pixels, at: &str) {
    let margin = if left {
        drawn.left()
    } else {
        width - drawn.right()
    };
    assert!(
        (margin - px(8.)).abs() < px(0.01),
        "{at}: the menu's drawn edge {drawn:?} stands {margin:?} from the window's side, not 8 px"
    );
}

/// Every menu's drawn edge, the ring outside its fill, not its fill, stands
/// 8 px from the window's side it is moved in from, at 13 and 18 pt in a
/// 461 × 490 window: a dropdown on a button at either side of the window, on
/// a whole pixel and half a pixel into one, a row's right-click menu opened
/// 3 px from either side, and a submenu that runs past either side. Moved in
/// from a side, a dropdown's and a row menu's fill stood 8 px in and their
/// edge 7 px, where the input's drawn menu kept its edge 8 px in, and a
/// submenu kept its fill only 4 px inside the window, and only once its
/// anchor ran past it. Moved by whole pixels from a button half a pixel into
/// one, a dropdown's edge then stood 8.5 px in.
#[gpui::test]
fn menus_keep_their_drawn_edge_8_px_from_the_window_sides(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    let edge = gpui_kit::component::POPOVER_EDGE_WIDTH;
    let width = px(461.);
    let long: &'static str = format!("menu-item: {LONG_ITEM} · MenuItem").leak();
    for text in [13, 18] {
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| SideMenusProbe);
            Root::new(probe, window, cx)
        });
        cx.update(|window, cx| {
            appearance::ThemeChoice::Midnight.apply(Some(window), cx);
            appearance::apply_text_sizes(text, 12, window, cx);
        });
        cx.simulate_resize(size(width, px(490.)));
        draw(cx);

        for (button, left) in [
            ("left-dropdown", true),
            ("right-dropdown", false),
            ("left-half-dropdown", true),
            ("right-half-dropdown", false),
        ] {
            let at = format!("{text} pt, the {button}");
            let trigger = cx.debug_bounds(button).expect(button);
            cx.simulate_click(trigger.center(), Modifiers::default());
            draw(cx);
            draw(cx);
            assert_side_margin(open_menu_surface(cx).dilate(edge), left, width, &at);
            cx.simulate_keystrokes("escape");
            draw(cx);
        }

        let row = cx.debug_bounds("side-row").expect("the row");
        for (x, left) in [(px(3.), true), (width - px(3.), false)] {
            let at = format!("{text} pt, the row's menu {x:?} across");
            let pointer = point(x, row.center().y);
            cx.simulate_mouse_down(pointer, MouseButton::Right, Modifiers::default());
            cx.simulate_mouse_up(pointer, MouseButton::Right, Modifiers::default());
            draw(cx);
            draw(cx);
            assert_side_margin(open_menu_surface(cx).dilate(edge), left, width, &at);
            cx.simulate_keystrokes("escape");
            draw(cx);
        }

        // The submenu of a menu at the left opens to its right and runs past
        // the window's right side; that of a menu at the right opens to its
        // left and runs past the window's left side.
        for (button, left) in [("left-nested", false), ("right-nested", true)] {
            let at = format!("{text} pt, the submenu of the {button}");
            let trigger = cx.debug_bounds(button).expect(button);
            cx.simulate_click(trigger.center(), Modifiers::default());
            draw(cx);
            draw(cx);
            let more = cx
                .debug_bounds("menu-item: More · MenuItem")
                .unwrap_or_else(|| panic!("{at}: the menu's More"));
            cx.simulate_mouse_move(more.center(), None, Modifiers::default());
            draw(cx);
            draw(cx);
            assert_side_margin(menu_surface_around(cx, long).dilate(edge), left, width, &at);
            cx.simulate_keystrokes("escape");
            draw(cx);
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }
}

/// A row's right-click menu opened 3 px from either side of a 460 px
/// window draws its edge, the ring outside its fill, 8 px from that side to
/// the device pixel at the fractional scales 1.25, as Omarchy sets, and 1.5:
/// 10 and 12 device pixels. Its fill, painted on the device pixels GPUI
/// places it on, and its ring over every device pixel the band outside the
/// fill touches (`paint_drop_shadows`) are read in device pixels. Clamped 9
/// px in, between device pixels at 1.25, and moved there inside an overlay
/// GPUI's anchor leaves half a device pixel off one, the fill lay on the
/// device pixel nearer the left side, its edge drawn from device pixel 9,
/// 7.2 px from the side.
#[gpui::test]
fn a_rows_menu_keeps_its_drawn_edge_8_px_from_the_sides_at_fractional_scales(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    let edge = f32::from(gpui_kit::component::POPOVER_EDGE_WIDTH);
    // A whole number of device pixels at each scale.
    let (width, height) = (px(460.), px(488.));
    for scale in [1.25, 1.5] {
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| SideMenusProbe);
            Root::new(probe, window, cx)
        });
        cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
        cx.simulate_resize(size(width, height));
        cx.update(|window, _| window.set_scale_factor(scale));
        draw(cx);
        let expected = f32::from(super::MENU_MARGIN) * scale;
        let row = cx.debug_bounds("side-row").expect("the row");
        for (x, left) in [(px(3.), true), (width - px(3.), false)] {
            let at = format!("scale {scale}, the row's menu {x:?} across");
            let pointer = point(x, row.center().y);
            cx.simulate_mouse_down(pointer, MouseButton::Right, Modifiers::default());
            cx.simulate_mouse_up(pointer, MouseButton::Right, Modifiers::default());
            draw(cx);
            draw(cx);
            let item = cx
                .debug_bounds("menu-item: One · MenuItem")
                .expect("the menu");
            let fill = cx.update(|window, cx| {
                let popover = Background::from(Theme::global(cx).popover);
                let center = point(
                    gpui_kit::ScaledPixels(f32::from(item.center().x) * scale),
                    gpui_kit::ScaledPixels(f32::from(item.center().y) * scale),
                );
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.background == popover && quad.bounds.contains(&center))
                    .max_by_key(|quad| quad.order)
                    .map(|quad| quad.bounds)
                    .unwrap_or_else(|| panic!("{at}: the menu's fill"))
            });
            let from_side = if left {
                (fill.left().as_f32() - edge * scale).floor()
            } else {
                f32::from(width) * scale - (fill.right().as_f32() + edge * scale).ceil()
            };
            assert_eq!(
                from_side, expected,
                "{at}: the fill {fill:?} draws its edge {from_side} device pixels from the side, \
                 not {expected}"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
    }
}

/// The open menu's fill, which holds its item One, as painted, in device
/// pixels: the last fill of the popover colour around that item.
fn open_menu_fill_in_device_pixels(cx: &mut VisualTestContext) -> Bounds<gpui_kit::ScaledPixels> {
    let item = cx
        .debug_bounds("menu-item: One · MenuItem")
        .expect("the menu");
    cx.update(|window, cx| {
        let popover = Background::from(Theme::global(cx).popover);
        let scale = window.scale_factor();
        let center = point(
            gpui_kit::ScaledPixels(f32::from(item.center().x) * scale),
            gpui_kit::ScaledPixels(f32::from(item.center().y) * scale),
        );
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background == popover && quad.bounds.contains(&center))
            .max_by_key(|quad| quad.order)
            .map(|quad| quad.bounds)
            .expect("the menu's fill")
    })
}

/// A row's right-click menu opened below the pointer draws its edge, the
/// ring outside its fill, from the device pixel row just below the
/// pointer's, never on it or above it, as a text field's drawn menu does:
/// read in device pixels as painted, its fill where GPUI places it and its
/// ring over every device pixel the band outside the fill touches, at
/// scales 1, 1.2 (as Hyprland gives a panel whose one mode it cannot set
/// to 1.25), 1.25 (as Omarchy sets), 1.5 and 2, for rows 40, 41 and 42 px
/// down and a pointer on a whole pixel, as X11 reports one, and part way
/// into one, as Wayland does. With its fill at the pointer, its edge lay on
/// the pixel row above the pointer's, over the row clicked; with its fill
/// rounded out to a whole logical pixel, at 1.2 and 1.25 the fill started
/// part way into a device pixel, which GPUI rounds back towards the
/// pointer, and the edge covered the pointer's own device row.
#[gpui::test]
fn a_rows_menu_below_the_pointer_draws_its_edge_below_the_pointers_row(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    let edge = f32::from(gpui_kit::component::POPOVER_EDGE_WIDTH);
    let mut failures = Vec::new();
    for scale in [1., 1.2, 1.25, 1.5, 2.] {
        for lead in [40., 41., 42.] {
            let (_, cx) = cx.add_window_view(|window, cx| {
                let probe = cx.new(|_| GapRowProbe { lead: px(lead) });
                Root::new(probe, window, cx)
            });
            cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            cx.update(|window, _| window.set_scale_factor(scale));
            draw(cx);
            let row = cx.debug_bounds("gap-row").expect("the row");
            for into in [0., 0.4, 0.7] {
                let at = format!("scale {scale}, a row {lead} px down, {into} px into a pixel");
                let pointer = point(row.left() + px(40.), row.top().floor() + px(8. + into));
                cx.simulate_mouse_down(pointer, MouseButton::Right, Modifiers::default());
                cx.simulate_mouse_up(pointer, MouseButton::Right, Modifiers::default());
                draw(cx);
                draw(cx);
                let fill = open_menu_fill_in_device_pixels(cx);
                // The device pixel row the pointer lies in, and the first
                // the menu's ring touches.
                let pointer_row = (f32::from(pointer.y) * scale + 1e-3).floor();
                let ring_row = (fill.top().as_f32() - edge * scale + 1e-3).floor();
                if ring_row != pointer_row + 1. {
                    failures.push(format!(
                        "{at}: the fill {fill:?} draws its edge from device row {ring_row}, \
                         the pointer {pointer:?} lying in row {pointer_row}"
                    ));
                }
                cx.simulate_keystrokes("escape");
                draw(cx);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A right-click's row that moves a fraction of a pixel, as a refresh can
/// lay it out, keeps its menu open where the menu fits on neither side:
/// moved 0.1 px across a pixel's edge, and 0.4 px within one, at scale 1
/// and 1.5. The menu's place below the pointer rounds out to a whole pixel,
/// and judged from that place, a 0.1 px move across a pixel's edge moved
/// it by a whole pixel and closed the menu.
#[test]
fn a_row_menu_stays_with_a_row_moved_a_fraction_of_a_pixel() {
    use super::{RowAnchor, leaves_its_row};
    let room = Bounds::from_corners(point(px(0.), px(9.)), point(px(400.), px(300.)));
    // Taller than the room on either side of a row near the room's middle.
    let menu = size(px(160.), px(280.));
    for scale in [1., 1.5] {
        // A pointer 0.95 px into a row's pixel at 100 px, and moved with
        // the row by `by`.
        let at = |by: f32| {
            let row = Bounds::new(point(px(10.), px(90. + by)), size(px(380.), px(24.)));
            RowAnchor::pointer(point(px(40.), px(100.95 + by)), row, scale)
        };
        let opened = at(0.);
        for by in [0.1, 0.4, -0.4] {
            assert!(
                !leaves_its_row(opened, (at(by), true), menu, room),
                "scale {scale}: a row moved {by} px keeps its menu"
            );
        }
        assert!(
            leaves_its_row(opened, (at(1.), true), menu, room),
            "scale {scale}: a row moved a whole pixel takes its menu with it"
        );
    }
}

/// A row `inset` in from the window's sides and `top` below its top, with
/// the app's right-click menu of four items. Layout puts those lengths on
/// device pixels, so at a fractional scale the row can start part way into
/// a logical pixel: 3 px in is 4 device pixels at 1.5, 2.67 px, where the
/// overlay GPUI's anchor moves by whole pixels lies half a device pixel off
/// the grid.
struct InsetRowProbe {
    inset: Pixels,
    top: Pixels,
}

impl Render for InsetRowProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().pt(self.top).px(self.inset).child(
            div()
                .id("inset-row")
                .debug_selector(|| "inset-row".into())
                .h(px(24.))
                .w_full()
                .child("A row")
                .app_context_menu(|menu, _, _| {
                    ["One", "Two", "Three", "Four"]
                        .into_iter()
                        .fold(menu, |menu, name| menu.item(PopupMenuItem::new(name)))
                }),
        )
    }
}

/// Where a menu's fill starts or ends, in device pixels, clamped to the
/// side of a window `extent` long at `start` or its far end on a display of
/// `scale`: on the first device pixel at or inside the margin and the
/// menu's edge (`super::menu_bounds`), so the edge outside it is drawn from
/// the device pixel holding the 8 px line.
fn clamped_fill(start: bool, extent: Pixels, scale: f32) -> f32 {
    let inside = f32::from(super::MENU_MARGIN + super::MENU_EDGE) * scale;
    if start {
        (inside - 1e-3).ceil()
    } else {
        (f32::from(extent) * scale - inside + 1e-3).floor()
    }
}

/// A row's right-click menu opened 3 px from either side of a 460 px
/// window, the row 3 px in from the window's sides too, draws its fill on
/// the first device pixel inside the 8 px margin and its edge, at the
/// fractional scales 1.2 (as Hyprland gives a panel it cannot set to 1.25),
/// 1.25 (as Omarchy sets) and 1.5: there the ring outside the fill starts
/// 8 px from the side, 10 and 12 device pixels, and at 1.2 on the device
/// pixel holding the 8 px line. The row at 3 px lies 4 device pixels in at
/// 1.5, 2.67 px, so the overlay GPUI's anchor moves by whole pixels lies
/// half a device pixel off the grid, which `RowMenu::prepaint` takes back
/// from a clamped menu: the side row at 2 px, 3 device pixels at 1.5, left
/// it nothing to correct there.
#[gpui::test]
fn a_rows_menu_off_the_pixel_grid_keeps_its_edge_8_px_from_the_sides(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    // A whole number of device pixels at each scale.
    let (width, height) = (px(460.), px(480.));
    let mut failures = Vec::new();
    for scale in [1.2, 1.25, 1.5] {
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| InsetRowProbe {
                inset: px(3.),
                top: px(41.),
            });
            Root::new(probe, window, cx)
        });
        cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
        cx.simulate_resize(size(width, height));
        cx.update(|window, _| window.set_scale_factor(scale));
        draw(cx);
        let row = cx.debug_bounds("inset-row").expect("the row");
        for (x, left) in [(row.left() + px(1.), true), (row.right() - px(1.), false)] {
            let at = format!("scale {scale}, the row {row:?}'s menu {x:?} across");
            let pointer = point(x, row.center().y);
            cx.simulate_mouse_down(pointer, MouseButton::Right, Modifiers::default());
            cx.simulate_mouse_up(pointer, MouseButton::Right, Modifiers::default());
            draw(cx);
            draw(cx);
            let fill = open_menu_fill_in_device_pixels(cx);
            let (drawn, expected) = if left {
                (fill.left().as_f32(), clamped_fill(true, width, scale))
            } else {
                (fill.right().as_f32(), clamped_fill(false, width, scale))
            };
            if drawn != expected {
                failures.push(format!(
                    "{at}: the fill {fill:?} lies at device pixel {drawn}, not {expected}"
                ));
            }
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The fill of the open menu whose item `selector` names, as painted, in
/// device pixels: the last fill of the popover colour around the item's
/// first device pixel, which lies inside the fill wherever the fill shows
/// any of the item, as a menu capped shorter than its row does.
fn menu_fill_at_item(
    cx: &mut VisualTestContext,
    selector: &'static str,
) -> Option<Bounds<gpui_kit::ScaledPixels>> {
    let item = cx.debug_bounds(selector)?;
    cx.update(|window, cx| {
        let popover = Background::from(Theme::global(cx).popover);
        let scale = window.scale_factor();
        let first = point(
            gpui_kit::ScaledPixels((f32::from(item.left()) * scale).floor() + 0.5),
            gpui_kit::ScaledPixels((f32::from(item.top()) * scale).floor() + 0.5),
        );
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background == popover && quad.bounds.contains(&first))
            .max_by_key(|quad| quad.order)
            .map(|quad| quad.bounds)
    })
}

/// A row's right-click menu that fits neither below its row nor wholly above
/// it, in a window too short for even one of its items on either side,
/// stands with its top clamped to the room's, its fill on the first device
/// pixel inside the 8 px margin and its edge, at the fractional scales 1.2
/// (as Hyprland gives a panel it cannot set to 1.25) and 1.25 (as Omarchy
/// sets): there the ring outside the fill starts 8 px from the window's
/// top, 10 device pixels, and at 1.2 on the device pixel holding the 8 px
/// line. The row lies 34 px down, 3 px in, off a whole pixel at both
/// scales, so the overlay GPUI's anchor moves by whole pixels lies off the
/// device pixel grid, half a device pixel at 1.25 (laid out 42 device
/// pixels down, 33.6 px, and moved by 34 px), which `RowMenu::prepaint`
/// takes back from a menu clamped to the top as from one clamped to a
/// side; at 1.2 it lies less than half one off, which rounding absorbs. A menu capped to the
/// larger room keeps whole rows within the cap, so only a room shorter than
/// one row clamps it: here about 19 px above the row and none below it.
#[gpui::test]
fn a_rows_menu_clamped_to_the_top_keeps_its_edge_8_px_from_it(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::set_overlay_room(cx);
    });
    // A whole number of device pixels at each scale.
    let (width, height) = (px(460.), px(80.));
    let mut failures = Vec::new();
    for scale in [1.2, 1.25] {
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| InsetRowProbe {
                inset: px(3.),
                top: px(34.),
            });
            Root::new(probe, window, cx)
        });
        cx.update(|window, cx| appearance::ThemeChoice::Midnight.apply(Some(window), cx));
        cx.simulate_resize(size(width, height));
        cx.update(|window, _| window.set_scale_factor(scale));
        draw(cx);
        let row = cx.debug_bounds("inset-row").expect("the row");
        let at = format!("scale {scale}, the row {row:?}");
        let pointer = point(row.left() + px(40.), row.center().y);
        cx.simulate_mouse_down(pointer, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(pointer, MouseButton::Right, Modifiers::default());
        draw(cx);
        draw(cx);
        let Some(fill) = menu_fill_at_item(cx, "menu-item: One · MenuItem") else {
            failures.push(format!("{at}: no menu's fill"));
            continue;
        };
        let expected = clamped_fill(true, height, scale);
        if fill.top().as_f32() != expected {
            failures.push(format!(
                "{at}: the fill {fill:?} starts at device pixel {}, not {expected}",
                fill.top().as_f32()
            ));
        }
        cx.simulate_keystrokes("escape");
        draw(cx);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A menu the keyboard opens (Shift-F10) on the changed file highlighted in
/// History's inspector, at 18 pt in a 460 × 480 and a 460 × 900 window, at
/// the scales 1, 1.2 (as Hyprland gives a panel it cannot set to 1.25),
/// 1.25 (as Omarchy sets), 1.5 and 2, lies on device pixels inside the room
/// the window leaves it: wider than the inspector beside the window's right
/// side, its fill is clamped to the first device pixel inside the 8 px
/// margin and its edge there, so the edge is 8 px from the side (10 device
/// pixels at 1.25, the device pixel holding the 8 px line at 1.2). Its
/// drawn edge stands the menu gap from its row rounded out to the device
/// pixel, the same way at every scale (`super::RowMenu::origin`): wholly
/// above the row in the short window, where the room below is too short
/// for it, its fill ends on the last device pixel at or before the row's
/// top less the gap and the edge, and the edge stands 6, 5.67, 6.2, 5.67
/// and 6 px from the row, never nearer than the 5.54 px gap; below it in
/// the tall one, its fill starts on the first device pixel at or after the
/// row's foot, the gap and the edge. Left where GPUI's rounding put it, the
/// fill above the row lay a device pixel nearer the row at 1.2, its edge
/// 4.83 px from it, and 5.4 and 5.5 px off at 1.25 and 2, and the fill
/// below it 5.4, 5 and 5.5 px off at 1.25, 1.5 and 2. Right-click menus
/// were walked at fractional scales; the keyboard's, placed from the list's
/// row rather than a pointer, by none.
#[gpui::test]
async fn a_keyboard_s_row_menu_at_a_fractional_scale_lies_on_device_pixels(
    cx: &mut TestAppContext,
) {
    use gpui_kit::base::{device_pixel_at_or_after, device_pixel_at_or_before};
    const HISTORY: &str = "menu-item: File history… · MenuItem";
    let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
    cx.update(|_, cx| {
        crate::shortcuts::bind_keys(cx);
        super::bind_keys(cx);
    });
    let width = px(460.);
    let mut failures = Vec::new();
    let scales = [1., 1.2, 1.25, 1.5, 2.];
    let walk = [px(480.), px(900.)]
        .into_iter()
        .flat_map(|height| scales.map(|scale| (height, scale)));
    for (height, scale) in walk {
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(width, height));
        cx.update(|window, _| window.set_scale_factor(scale));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(!app.files.is_empty(), "the commit's changed files");
                app.selected_file = Some(0);
                app.pane = crate::Pane::Files;
                window.focus(&app.file_focus, cx);
                cx.notify();
            })
        });
        draw(cx);
        let row = cx
            .debug_bounds("selected-changed-file")
            .expect("the highlighted row");
        let at = format!("460 × {height:?} at scale {scale}, the row {row:?}");
        cx.simulate_keystrokes("shift-f10");
        draw(cx);
        draw(cx);
        let Some(fill) = menu_fill_at_item(cx, HISTORY) else {
            failures.push(format!("{at}: the file's menu and its fill"));
            continue;
        };
        let (right, last) = (fill.right().as_f32(), clamped_fill(false, width, scale));
        if right != last {
            failures.push(format!(
                "{at}: the fill {fill:?} ends at device pixel {right}, not {last}, clamped \
                 to the right side"
            ));
        }
        if fill.left().as_f32() < clamped_fill(true, width, scale) {
            failures.push(format!("{at}: the fill {fill:?} runs past the left side"));
        }
        // The fill's side toward the row lies on the device pixel the gap
        // and the edge end on, rounded away from the row, and the drawn edge,
        // `MENU_EDGE` outside the fill, stands the gap from the row, or
        // less than a device pixel further.
        let (top, bottom) = (fill.top().as_f32(), fill.bottom().as_f32());
        let (offset, edge) = (super::menu_offset(), super::MENU_EDGE);
        let below = top >= f32::from(row.bottom()) * scale;
        if below != (height > px(480.)) {
            failures.push(format!(
                "{at}: the fill {fill:?} lies on the other side of its row, so the walk \
                 misses a side"
            ));
        }
        let (drawn, place, gap) = if below {
            let place = device_pixel_at_or_after(row.bottom() + offset, scale);
            (top, place, px(top / scale) - edge - row.bottom())
        } else {
            let place = device_pixel_at_or_before(row.top() - offset, scale);
            (bottom, place, row.top() - px(bottom / scale) - edge)
        };
        let place = (f32::from(place) * scale).round();
        let popup_gap = appearance::popup_gap();
        if drawn != place || gap < popup_gap - px(1e-3) || gap >= popup_gap + px(1. / scale) {
            failures.push(format!(
                "{at}: the fill {fill:?} meets its row at device pixel {drawn}, not {place}, \
                 and draws its edge {gap:?} from the row, not the {popup_gap:?} gap rounded \
                 out to a device pixel"
            ));
        }
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(cx.debug_bounds(HISTORY).is_none(), "{at}: Escape closed it");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    cx.update(|window, _| window.set_scale_factor(2.));
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
}
