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
