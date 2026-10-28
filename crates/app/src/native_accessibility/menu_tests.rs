//! The kit's popup menu entries as the app's menus build them: a label that
//! titles the menu, a disabled item, available items, a separator and a
//! closing caption, as the project pane, profile and review Options menus do.
//! GPUI's test platform never activates accessibility, so the kit names the
//! AccessKit node each entry writes in its debug selector.
use crate::*;
use core::prelude::v1::test;
use gpui_kit::component::{
    Theme,
    menu::{DropdownMenu as _, PopupMenuItem},
};
use std::{cell::RefCell, rc::Rc};

/// The items chosen from the probe's menu, in order.
type Chosen = Rc<RefCell<Vec<&'static str>>>;

struct MenuProbe(Chosen);

impl Render for MenuProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.0.clone();
        div().size_full().p_4().child(
            Button::new("menu-probe")
                .debug_selector(|| "menu-probe".into())
                .label("Open")
                .dropdown_menu(move |menu, _, _| {
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
                        .label("Original Git diff")
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
            let probe = cx.new(|_| MenuProbe(chosen));
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
        ("Original Git diff", "menu-item: Original Git diff · Label"),
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
        "menu-item: Original Git diff · Label",
        "menu-item: Unavailable · MenuItem disabled",
        "menu-item: First · MenuItem",
        "menu-item: Second · MenuItem",
    ] {
        assert!(shown(cx, entry.into()).is_some(), "{entry}");
    }
    for entry in [
        "menu-item: Saved profiles · MenuItem disabled",
        "menu-item: Saved profiles · Label disabled",
        "menu-item: Original Git diff · MenuItem disabled",
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
        for label in ["Saved profiles", "Original Git diff"] {
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
    let labels = ["Saved profiles", "Original Git diff"];
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
