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
    let item = cx
        .debug_bounds("menu-item: One · MenuItem")
        .expect("the menu's first item");
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
        super::keep_popups_above_the_status_bar(cx);
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
        super::keep_popups_above_the_status_bar(cx);
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
/// or leaves the part of its list drawn: clamped to the room, it lay over
/// the row it acts on. Moved where it still fits on either side, or never
/// fitting since it opened, it stays.
#[test]
fn a_row_menu_leaves_a_row_moved_where_it_no_longer_fits() {
    use super::{RowAnchor, leaves_its_row};
    let room = Bounds::from_corners(point(px(0.), px(9.)), point(px(400.), px(300.)));
    let row = |top: f32| RowAnchor {
        x: px(10.),
        top: px(top),
        below: px(top + 44.),
        trailing: None,
    };
    let (menu, opened) = (size(px(160.), px(150.)), (row(100.), true));
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
        "never fitting"
    );
    assert!(
        !leaves_its_row((row(300.), false), (row(300.), false), menu, room),
        "opened for a row the list did not draw"
    );
}
