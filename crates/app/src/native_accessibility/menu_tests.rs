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

/// The entry the probe menu highlights, by the label of the item whose
/// bounds hold the highlight's fill, or `None` when nothing is highlighted.
fn highlighted(cx: &mut VisualTestContext) -> Option<&'static str> {
    let highlight = cx.update(|window, cx| {
        let fill = Background::from(Theme::global(cx).tokens.accent.color);
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
