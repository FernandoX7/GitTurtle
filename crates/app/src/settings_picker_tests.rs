use super::*;
use core::prelude::v1::test;
use std::{cell::RefCell, rc::Rc};

pub(super) fn open_app(cx: &mut TestAppContext) -> (Entity<GitTurtle>, &mut VisualTestContext) {
    open_styled_app(cx, |root| root)
}

/// [`open_app`] with its window's root styled by `style`, as a test that
/// reads where text runs are drawn marks them (`width_walk::text_mark`).
fn open_styled_app(
    cx: &mut TestAppContext,
    style: impl FnOnce(gpui_kit::component::Root) -> gpui_kit::component::Root + 'static,
) -> (Entity<GitTurtle>, &mut VisualTestContext) {
    // GitTurtle::new starts a real preferences worker; saves reply from it.
    cx.executor().allow_parking();
    crate::views::tests::init_app(cx);
    cx.update(|cx| {
        theme_editor::init(cx);
    });
    let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
    let output = captured.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| {
            GitTurtle::new(
                None,
                Preferences::default(),
                repository_tabs::Session::default(),
                activity::State::default(),
                recovery_drafts::State::default(),
                window,
                cx,
            )
        });
        *output.borrow_mut() = Some(app.clone());
        style(gpui_kit::component::Root::new(app, window, cx))
    });
    let app = captured.borrow().as_ref().unwrap().clone();
    // Tall enough that the Your themes card is on screen without scrolling.
    cx.simulate_resize(size(px(1440.), px(2400.)));
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.apply_appearance(window, cx);
            app.show_settings(window, cx)
        })
    });
    settle(cx);
    (app, cx)
}

pub(super) fn settle(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
}

/// In a short window an assistive technology reaches only the Settings
/// controls the window draws: no reachable control lies wholly outside
/// its nearest clipping ancestor, beyond the one neighbour AccessKit
/// keeps past each edge for it to move into, and that ancestor is the
/// part of the page the window draws. Settings' page did not clip, so
/// every theme card, switch and button below the window was reached.
///
/// The test platform builds no accessibility tree, so the test builds
/// the part GPUI would: the wrappers' nodes as they were drawn, and the
/// controls, as the page's children, at their bounds, in the order the
/// page draws them. It walks the tree through AccessKit's own filter.
#[gpui::test]
async fn assistive_technology_reaches_only_the_settings_drawn(cx: &mut TestAppContext) {
    use crate::native_accessibility::{reachable, written};
    use gpui::accesskit::{Node, Rect};

    let (_app, cx) = open_app(cx);
    let mut selectors: Vec<String> = ThemeChoice::ALL
        .iter()
        .map(|choice| format!("settings-theme-{}", *choice as usize))
        .collect();
    selectors.extend((0..4).map(|index| format!("settings-density-{index}")));
    selectors.extend(
        [
            "settings-theme-omarchy",
            "follow-system-switch",
            "code-font-switch",
            "custom-themes-new",
            "custom-themes-import",
            "save-external-editor",
            "save-external-terminal",
            "save-default-branch",
        ]
        .map(str::to_owned),
    );
    let physical = |cx: &mut VisualTestContext, bounds: Bounds<Pixels>| {
        let scale = cx.update(|window, _| f64::from(window.scale_factor()));
        Rect {
            x0: f64::from(f32::from(bounds.left())) * scale,
            y0: f64::from(f32::from(bounds.top())) * scale,
            x1: f64::from(f32::from(bounds.right())) * scale,
            y1: f64::from(f32::from(bounds.bottom())) * scale,
        }
    };
    let meets = |a: Rect, b: Rect| !a.intersect(b).is_empty();
    let close = |a: Rect, b: Rect| {
        [(a.x0, b.x0), (a.y0, b.y0), (a.x1, b.x1), (a.y1, b.y1)]
            .iter()
            .all(|(a, b)| (a - b).abs() < 0.01)
    };

    for (width, height) in [(640., 490.), (461., 490.)] {
        let at = format!("{width} × {height}");
        cx.simulate_resize(size(px(width), px(height)));
        settle(cx);
        // Without an assistive technology the page is a cached view,
        // which a frame that changes nothing reuses unseen; it is not
        // cached while one is active, so draw it again whole.
        written::forget();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let window = physical(
            cx,
            Bounds::new(point(px(0.), px(0.)), size(px(width), px(height))),
        );
        // The part of the page the window draws, from layout alone.
        let page = cx
            .debug_bounds("settings-scroll")
            .expect("Settings is drawn");
        let drawn = physical(
            cx,
            page.intersect(&Bounds::new(
                point(px(0.), px(0.)),
                size(px(width), px(height)),
            )),
        );
        // The controls, in the order the page draws them.
        let mut controls: Vec<(String, Rect)> = selectors
            .iter()
            .filter_map(|selector| {
                cx.debug_bounds(selector.clone().leak())
                    .map(|bounds| (selector.clone(), physical(cx, bounds)))
            })
            .collect();
        controls.sort_by(|a, b| (a.1.y0, a.1.x0).partial_cmp(&(b.1.y0, b.1.x0)).unwrap());
        assert!(
            controls.len() > 20 && controls.iter().any(|(_, bounds)| !meets(*bounds, drawn)),
            "at {at} the page runs past the window: {controls:?}"
        );

        let mut tree = written::Assembly::default().written("settings-scroll", None);
        // Without a node of its own the page's controls are children of
        // the node around it, as they were.
        let parent = if tree.has("settings-scroll") {
            "settings-scroll"
        } else {
            tree = tree.written("gitturtle-page", None);
            "gitturtle-page"
        };
        for (selector, bounds) in &controls {
            let mut control = Node::new(Role::Button);
            control.set_label(selector.clone());
            control.set_bounds(*bounds);
            tree = tree.node(selector, control, Some(parent));
        }
        let found = reachable(tree.tree(window, None));
        let reached: Vec<_> = found
            .iter()
            .filter(|node| {
                node.label
                    .as_ref()
                    .is_some_and(|label| selectors.contains(label))
            })
            .collect();
        for node in &reached {
            let bounds = node.bounds.expect("bounds");
            let clip = node.clip.unwrap_or(window);
            let index = controls
                .iter()
                .position(|(selector, _)| Some(selector) == node.label.as_ref())
                .unwrap();
            // AccessKit keeps the first control past each edge.
            let neighbour = [index.checked_sub(1), Some(index + 1)]
                .into_iter()
                .flatten()
                .filter_map(|other| controls.get(other))
                .any(|(_, other)| meets(*other, clip));
            assert!(
                meets(bounds, clip) || neighbour,
                "at {at} {:?} at {bounds:?} is reached, wholly outside its clipping ancestor {clip:?}",
                node.label
            );
        }
        // The clipping ancestor is the part of the page the window draws.
        for node in &reached {
            assert!(
                node.clip.is_some_and(|clip| close(clip, drawn)),
                "at {at} {:?} is clipped to {:?}; the window draws {drawn:?} of the page",
                node.label,
                node.clip
            );
        }
        // Every control the window draws is in reach.
        for (selector, bounds) in &controls {
            if meets(*bounds, drawn) {
                assert!(
                    reached
                        .iter()
                        .any(|node| node.label.as_ref() == Some(selector)),
                    "at {at} the drawn {selector} is in reach"
                );
            }
        }
    }
}

fn seed_themes(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, count: u32) {
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            let themes = (1..=count)
                .map(|n| CustomTheme::from_base(n, format!("Seed {n:02}"), ThemeChoice::Nord))
                .collect();
            app.set_custom_themes(themes, cx);
            cx.notify();
        })
    });
    settle(cx);
}

/// The miniature `selection`'s picker card embeds, which names the
/// selectors of its caption.
fn miniature(
    cx: &mut VisualTestContext,
    app: &Entity<GitTurtle>,
    selection: ThemeSelection,
) -> EntityId {
    cx.read(|cx| app.read(cx).theme_preview_body(selection).entity_id())
}

/// With a repository open, the Git identity's Name and Email labels are
/// medium, as the Projects setting titles beside them are, rather than
/// regular, and every theme card's name is medium rather than semibold.
/// On Linux the bundled interface font draws each weight in a face of its
/// own, so the difference shows.
#[gpui::test]
fn settings_labels_draw_in_medium(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let repo = crate::tags::tests::tagged_repository(fixture.path());
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| theme_editor::init(cx));
    // Tall enough that every card is on screen without scrolling.
    cx.simulate_resize(size(px(1440.), px(2400.)));
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(cx);
    for label in ["Name", "Email"] {
        assert_eq!(
            appearance::drawn_weight(&format!("settings-field-label: {label}")),
            Some(FontWeight::MEDIUM),
            "the {label} label"
        );
    }
    for choice in ThemeChoice::ALL {
        let body = miniature(cx, &app, ThemeSelection::BuiltIn(choice));
        assert_eq!(
            appearance::drawn_weight(&format!("theme-name-{body}")),
            Some(FontWeight::MEDIUM),
            "the {} card's name",
            choice.label()
        );
    }
}

fn drawn(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    shown(cx, selector.leak())
}

/// The theme grid takes its columns from the width the page is drawn at,
/// not the window's. Beside the 240 px project list a 1,200 px window
/// leaves the page 960 px, below the 1,060 px breakpoint, so the dark
/// palettes' first row holds three cards, as a 960 px window without the
/// list does; at 18 pt a 1,480 px window, over the scaled breakpoint,
/// leaves the page 1,240 px, under it, and three as well. The grid laid
/// four out for the window's width in the room the page did not have.
#[gpui::test]
fn the_theme_grid_takes_its_columns_from_the_pages_width(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    let dark: Vec<_> = ThemeChoice::ALL
        .into_iter()
        .filter(|choice| !choice.is_light())
        .collect();
    let first_row = |cx: &mut VisualTestContext| {
        let card = |cx: &mut VisualTestContext, choice: ThemeChoice| {
            drawn(cx, format!("settings-theme-{}", choice as usize))
        };
        let top = card(cx, dark[0]).expect("the first dark card").top();
        dark.iter()
            .filter(|choice| card(cx, **choice).is_some_and(|card| card.top() == top))
            .count()
    };
    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    let text_size = |cx: &mut VisualTestContext, interface: u8| {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
    };
    for (interface, width) in [(13, 1200.), (18, 1480.)] {
        let at = format!("{interface} pt in a {width} px window");
        text_size(cx, interface);
        cx.simulate_resize(size(px(width), px(2400.)));
        settle(cx);
        assert!(px(width) >= appearance::ui_size(1060.), "{at}");
        assert_eq!(first_row(cx), 4, "{at} without the project list");
        cx.update(|window, cx| app.update(cx, |app, cx| app.toggle_project_pane(window, cx)));
        settle(cx);
        let page = drawn(cx, "settings-scroll".into()).expect("the page");
        assert!(
            page.size.width <= px(width - 240.) + px(0.5)
                && page.size.width < appearance::ui_size(1060.),
            "{at}: the page {page:?} beside the project list"
        );
        assert_eq!(
            first_row(cx),
            3,
            "{at}: beside the project list, the {:?} page's columns",
            page.size.width
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.toggle_project_pane(window, cx)));
        settle(cx);
    }
    text_size(cx, 13);
    settle(cx);
}

/// The check badge is the name's line tall at every interface text size:
/// the selected card's caption, which here
/// also holds the warning glyph, starts level with its row neighbours'
/// at the smallest, the default and the largest sizes.
#[gpui::test]
fn a_selected_caption_starts_level_with_its_row(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    let mut dawn = CustomTheme::from_base(1, "Dawn Chorus", ThemeChoice::RosePineDawn);
    dawn.palette.muted = dawn.palette.canvas;
    assert!(!dawn.palette.readability_issues().is_empty());
    let themes = std::iter::once(dawn)
        .chain(
            (2..=4).map(|id| CustomTheme::from_base(id, format!("Seed {id}"), ThemeChoice::Nord)),
        )
        .collect();
    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.set_custom_themes(themes, cx);
            app.settings.theme = ThemeSelection::Custom(1);
            app.apply_appearance(window, cx);
        })
    });
    settle(cx);
    let selected = miniature(cx, &app, ThemeSelection::Custom(1));
    for interface in [11, 12, 13, 18] {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        settle(cx);
        let badge = drawn(cx, format!("theme-check-{selected}"))
            .unwrap_or_else(|| panic!("at {interface} pt the selected card has its badge"));
        let name = drawn(cx, format!("theme-name-{selected}")).expect("a name");
        assert_eq!(
            badge.size.height, name.size.height,
            "at {interface} pt the badge is the name's line"
        );
        assert!(drawn(cx, format!("theme-warning-{selected}")).is_some());
        let card = |cx: &mut VisualTestContext, id: u32| {
            drawn(cx, format!("settings-custom-theme-{id}")).expect("the card is drawn")
        };
        let caption = drawn(cx, format!("theme-caption-{selected}")).expect("a caption");
        // Four columns fit at 1,440 px up to the scaled breakpoint and
        // three at 18 pt, so Seed 4 may start the next row.
        let row = card(cx, 1).top();
        let beside = (2..=4)
            .filter(|id| card(cx, *id).top() == row)
            .collect::<Vec<_>>();
        assert!(beside.len() >= 2, "at {interface} pt: {beside:?}");
        for id in beside {
            let neighbour = miniature(cx, &app, ThemeSelection::Custom(id));
            assert!(drawn(cx, format!("theme-check-{neighbour}")).is_none());
            let beside = drawn(cx, format!("theme-caption-{neighbour}")).expect("a caption");
            assert_eq!(
                caption.top(),
                beside.top(),
                "at {interface} pt the selected caption {caption:?} starts level with \
                 Seed {id}'s {beside:?}"
            );
        }
    }
}

/// A picker card that holds keyboard focus draws a 2 px accent ring 1 px
/// outside its border, clear of every other card, so focus shows on the
/// selected card, whose border is already accent. Only the focused card
/// draws it, and only while the keyboard was the last input.
#[gpui::test]
fn keyboard_focus_rings_the_picker_card_outside_its_border(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    cx.update(|window, _| window.activate_window());
    let selected = cx
        .read(|cx| app.read(cx).selected_theme_card(cx))
        .expect("a card is selected");
    let selector = |selection: ThemeSelection| match selection {
        ThemeSelection::BuiltIn(choice) => format!("settings-theme-{}", choice as usize),
        ThemeSelection::Custom(id) => format!("settings-custom-theme-{id}"),
        ThemeSelection::Omarchy => "settings-theme-omarchy".into(),
    };
    let focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            ThemeChoice::ALL
                .into_iter()
                .map(ThemeSelection::BuiltIn)
                .find(|choice| app.read(cx).theme_card_focus(*choice).is_focused(window))
        })
    };
    let ring = |cx: &mut VisualTestContext| {
        page_shows(cx, "theme-card-focus-ring");
        cx.debug_bounds("theme-card-focus-ring")
    };
    let device = cx.update(|window, _| px(1. / window.scale_factor()));
    let around = |ring: Bounds<Pixels>, card: Bounds<Pixels>| {
        let outset = card.dilate(px(3.));
        [
            (ring.left(), outset.left()),
            (ring.top(), outset.top()),
            (ring.right(), outset.right()),
            (ring.bottom(), outset.bottom()),
        ]
        .into_iter()
        .all(|(edge, expected)| (edge - expected).abs() <= device)
    };

    // Tab away from the selected card and back: keyboard focus on it.
    let handle = cx.read(|cx| app.read(cx).theme_card_focus(selected).clone());
    cx.update(|window, cx| window.focus(&handle, cx));
    settle(cx);
    cx.simulate_keystrokes("tab");
    cx.simulate_keystrokes("shift-tab");
    settle(cx);
    assert_eq!(focused(cx), Some(selected));
    let card = drawn(cx, selector(selected)).expect("the selected card is drawn");
    let drawn_ring = ring(cx).expect("the focused selected card draws the ring");
    assert!(around(drawn_ring, card), "{drawn_ring:?} around {card:?}");
    let clear = |cx: &mut VisualTestContext, ring: Bounds<Pixels>| {
        for other in ThemeChoice::ALL.map(ThemeSelection::BuiltIn) {
            if other != selected {
                let other = drawn(cx, selector(other)).expect("every card is drawn");
                assert!(
                    !ring.intersects(&other),
                    "{ring:?} stays in the gap beside {other:?}"
                );
            }
        }
    };
    clear(cx, drawn_ring);

    // Tab on: the ring follows focus and leaves the selected card.
    cx.simulate_keystrokes("tab");
    settle(cx);
    let next = focused(cx).expect("Tab reaches the next card");
    assert_ne!(next, selected);
    let next_card = drawn(cx, selector(next)).expect("the next card is drawn");
    let drawn_ring = ring(cx).expect("the focused card draws the ring");
    assert!(
        around(drawn_ring, next_card),
        "{drawn_ring:?} around {next_card:?}"
    );
    assert!(!around(drawn_ring, card));
    assert_eq!(
        drawn(cx, selector(selected)),
        Some(card),
        "the ring's box does not move the card it holds"
    );

    // A click moves no focus (the kit's buttons refuse it on mouse down),
    // and the ring, which marks keyboard focus, goes.
    cx.simulate_click(card.center(), Modifiers::default());
    settle(cx);
    assert_eq!(focused(cx), Some(next));
    assert_eq!(ring(cx), None);

    // The compact grid, three columns with the same gaps.
    cx.simulate_resize(size(px(1000.), px(2400.)));
    settle(cx);
    let resting = drawn(cx, selector(selected)).expect("the selected card is drawn");
    cx.simulate_keystrokes("shift-tab");
    settle(cx);
    assert_eq!(focused(cx), Some(selected));
    assert_eq!(drawn(cx, selector(selected)), Some(resting));
    let drawn_ring = ring(cx).expect("the focused selected card draws the ring");
    assert!(
        around(drawn_ring, resting),
        "{drawn_ring:?} around {resting:?}"
    );
    clear(cx, drawn_ring);
}

/// A native dialog's failure on the Your themes card is one text: the
/// guidance in the warning color and the service error it quotes on its
/// second line in muted text, secondary to it. Any other failure is one
/// warning run.
#[gpui::test]
fn a_dialog_failure_mutes_the_service_error_it_quotes(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    let (sender, receiver) = futures::channel::oneshot::channel();
    sender
        .send(Err(anyhow::anyhow!(
            "org.freedesktop.zbus.Error: The name org.freedesktop.portal.Desktop \
             was not provided by any .service files"
        )
        .context("Portal request failed")))
        .unwrap();
    let failure = futures::executor::block_on(folder_picker::selected_path_for(
        folder_picker::Picker::File,
        receiver,
    ))
    .unwrap_err();
    let start = failure
        .find('\n')
        .expect("the quoted error has its own line")
        + 1;
    assert!(failure[start..].starts_with("Details: Portal request failed"));
    assert_eq!(folder_picker::detail_start(&failure), Some(start));
    let p = cx.read(palette);
    assert_ne!(p.warning, p.muted);
    for (error, selector) in [
        (
            failure,
            format!(
                "custom-themes-error-{:06x}-{start}-{:06x}",
                p.warning, p.muted
            ),
        ),
        (
            "Could not read the theme file: not JSON\nat line 1".to_owned(),
            format!("custom-themes-error-{:06x}", p.warning),
        ),
    ] {
        cx.update(|_, cx| app.update(cx, |app, _| app.theme_editor.set_error(error.clone())));
        assert!(
            page_shows(cx, selector.clone().leak()),
            "{selector} for {error:?}"
        );
    }
}

/// Linux: the switch saves the choice and asks the desktop-text worker for
/// the family (none runs here, so the lookup stays pending); off restores
/// the bundled font at once.
#[cfg(target_os = "linux")]
#[gpui::test]
fn the_code_font_switch_saves_the_choice_and_starts_the_lookup(cx: &mut TestAppContext) {
    use crate::desktop_text::{self, SystemCodeFont};

    let (app, cx) = open_app(cx);
    let state = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            (
                app.read(cx).settings.system_code_font,
                desktop_text::system_code_font(cx),
                gpui_kit::component::Theme::global(cx)
                    .mono_font_family
                    .to_string(),
            )
        })
    };
    let toggle = |cx: &mut VisualTestContext| {
        let switch = shown(cx, "code-font-switch").expect("Linux Settings offers the switch");
        cx.simulate_click(switch.center(), Modifiers::default());
        settle(cx);
    };
    let bundled = crate::mono().to_string();
    assert_eq!(state(cx), (false, SystemCodeFont::Off, bundled.clone()));
    toggle(cx);
    assert_eq!(state(cx), (true, SystemCodeFont::Pending, bundled.clone()));
    toggle(cx);
    assert_eq!(state(cx), (false, SystemCodeFont::Off, bundled));
}

/// Linux: the row's description, read from its accessible name, names the
/// bundled font while off, the lookup while pending, the desktop's family
/// once found, and why the bundled font stays when the family is unusable.
/// A late reply for an earlier lookup is dropped: after the setting turned
/// off the row stays off, and once it is on again the row stays pending
/// until the current lookup answers.
#[cfg(target_os = "linux")]
#[gpui::test]
fn the_code_font_row_describes_off_pending_and_on(cx: &mut TestAppContext) {
    use crate::desktop_text;

    let (_app, cx) = open_app(cx);
    let describes = |cx: &mut VisualTestContext, description: &str| {
        // The row's description is its switch's accessible description.
        assert!(page_shows(cx, "code-font-switch"));
        let built = switch_built("settings-system-code-font").map(|(_, built)| built);
        let state = cx.read(desktop_text::system_code_font);
        assert_eq!(
            built.as_deref(),
            Some(description),
            "the switch's description while {state:?}"
        );
    };
    let toggle = |cx: &mut VisualTestContext| {
        let switch = shown(cx, "code-font-switch").expect("Linux Settings offers the switch");
        cx.simulate_click(switch.center(), Modifiers::default());
        settle(cx);
    };
    // The lookup a worker started now would answer for.
    let lookup = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| desktop_text::code_font_lookup_generation(cx))
    };
    let answer =
        |cx: &mut VisualTestContext, generation: u64, found: Result<SharedString, String>| {
            let changed =
                cx.update(|_, cx| desktop_text::answer_code_font_lookup(generation, found, cx));
            settle(cx);
            changed
        };
    let bundled = crate::mono();
    let off = format!(
        "Code uses the bundled {bundled}. Turn on to use fontconfig's monospace font when it is fixed-width."
    );
    let pending = "Looking up the desktop's monospace font…";
    describes(cx, &off);
    toggle(cx);
    describes(cx, pending);
    let first = lookup(cx);
    assert_ne!(first, 0, "a lookup runs while the setting is on");
    assert!(answer(cx, first, Ok("Desktop Mono".into())));
    describes(cx, "Code uses Desktop Mono, the desktop's monospace font.");

    // The first lookup answers again after the setting turned off.
    toggle(cx);
    describes(cx, &off);
    assert!(!answer(cx, first, Ok("Other Mono".into())));
    describes(cx, &off);

    // On again: the first lookup's reply is stale, the second's is shown.
    toggle(cx);
    let second = lookup(cx);
    assert_ne!(second, first);
    assert!(!answer(cx, first, Ok("Desktop Mono".into())));
    describes(cx, pending);
    let reason = "Proportional Sans is not monospace.";
    assert!(answer(cx, second, Err(reason.into())));
    describes(cx, &format!("{reason} Code uses the bundled {bundled}."));
}

/// Linux: with no fontconfig cache the lookup outlives the earlier 500 ms
/// bound. The row stays pending and then shows the family, which code
/// uses; a lookup for a setting since turned off and on stops, and its
/// late answer is dropped; and one that outlives the lookup's bound says
/// why the bundled font stays. The lookups are the worker's own, with a
/// stand-in for `fc-match` on the test clock, and each process is reaped.
#[cfg(target_os = "linux")]
#[gpui::test]
fn the_code_font_row_stays_pending_through_a_slow_lookup(cx: &mut TestAppContext) {
    use crate::desktop_text::{self, CODE_FONT_WAIT, StandIn, SystemCodeFont};
    use std::time::Duration;

    let (_app, cx) = open_app(cx);
    let describes = |cx: &mut VisualTestContext, description: &str| {
        assert!(page_shows(cx, "code-font-switch"));
        let built = switch_built("settings-system-code-font").map(|(_, built)| built);
        let state = cx.read(desktop_text::system_code_font);
        assert_eq!(
            built.as_deref(),
            Some(description),
            "the switch's description while {state:?}"
        );
    };
    let toggle = |cx: &mut VisualTestContext| {
        let switch = shown(cx, "code-font-switch").expect("Linux Settings offers the switch");
        cx.simulate_click(switch.center(), Modifiers::default());
        settle(cx);
    };
    let lookup = |cx: &mut VisualTestContext, fake: &StandIn| {
        let program = fake.program();
        cx.update(|_, cx| desktop_text::run_code_font_lookup(program, cx))
    };
    let until = |cx: &mut VisualTestContext, done: &dyn Fn(&mut VisualTestContext) -> bool| {
        let started = std::time::Instant::now();
        while !done(cx) {
            assert!(started.elapsed() < Duration::from_secs(30), "timed out");
            desktop_text::pass_real_time(&cx.executor());
            settle(cx);
        }
    };
    // A lookup's process starts with the clock held, so however slowly a
    // loaded host starts it, the wait spends none of the lookup's bound.
    let started = |cx: &mut VisualTestContext, fake: &StandIn| {
        let since = std::time::Instant::now();
        while fake.code_font_queries() != 1 {
            assert!(since.elapsed() < Duration::from_secs(30), "timed out");
            desktop_text::pass_real_time_only(&cx.executor());
            settle(cx);
        }
    };
    let pass = |cx: &mut VisualTestContext, time: Duration| {
        cx.executor().advance_clock(time);
        settle(cx);
    };
    let code_family = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            gpui_kit::component::Theme::global(cx)
                .mono_font_family
                .to_string()
        })
    };
    let bundled = crate::mono();
    let pending = "Looking up the desktop's monospace font…";

    toggle(cx);
    let slow = StandIn::new();
    let _slow = lookup(cx, &slow);
    started(cx, &slow);
    pass(cx, Duration::from_millis(1500));
    describes(cx, pending);
    slow.answer(bundled);
    until(cx, &|cx| {
        cx.read(desktop_text::system_code_font) != SystemCodeFont::Pending
    });
    describes(
        cx,
        &format!("Code uses {bundled}, the desktop's monospace font."),
    );
    assert_eq!(
        cx.read(desktop_text::system_code_font),
        SystemCodeFont::Loaded(bundled.into())
    );
    assert_eq!(code_family(cx), bundled);
    assert!(slow.reaped());

    // Off and on while a lookup runs: it stops, and the earlier
    // setting's late answer is dropped.
    toggle(cx);
    toggle(cx);
    let stale = cx.update(|_, cx| desktop_text::code_font_lookup_generation(cx));
    let stopped = StandIn::new();
    let _stopped = lookup(cx, &stopped);
    started(cx, &stopped);
    toggle(cx);
    toggle(cx);
    until(cx, &|_| stopped.reaped());
    stopped.answer(bundled);
    pass(cx, Duration::from_millis(100));
    describes(cx, pending);
    assert!(!cx.update(|_, cx| {
        desktop_text::answer_code_font_lookup(stale, Ok("Desktop Mono".into()), cx)
    }));
    settle(cx);
    describes(cx, pending);

    // A lookup that never answers is unavailable at the bound, not before.
    let silent = StandIn::new();
    let _silent = lookup(cx, &silent);
    started(cx, &silent);
    pass(cx, CODE_FONT_WAIT - Duration::from_millis(50));
    describes(cx, pending);
    pass(cx, Duration::from_millis(50));
    describes(
        cx,
        &format!("fontconfig did not answer. Code uses the bundled {bundled}."),
    );
    assert_eq!(code_family(cx), bundled);
    assert!(silent.reaped());
}

/// Linux: with the desktop's Omarchy theme read, Settings offers it as the
/// one card of a Desktop group above the palettes, named "Omarchy theme",
/// its caption naming the desktop's theme and its miniature drawing the
/// mapped palette. Choosing it applies that palette, turns Follow system
/// off and locks it with the reason; choosing another card releases it.
/// The card stays while it is selected even when the file is gone, and
/// says why; unselected, it goes with the file.
#[cfg(target_os = "linux")]
#[gpui::test]
fn the_omarchy_card_selects_and_locks_follow_system(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("current");
    std::fs::create_dir_all(root.join("theme")).unwrap();
    let colors = include_bytes!("../tests/fixtures/omarchy/tokyo-night.toml");
    std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
    std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
    cx.update(|cx| drop(appearance::omarchy::install(Some(root.clone()), cx)));
    let (app, cx) = open_app(cx);
    settle(cx);
    let omarchy = ThemeSelection::Omarchy;
    let mapped = cx.read(|cx| appearance::omarchy::card(cx).unwrap().palette);
    assert_ne!(mapped, ThemeChoice::default().palette());

    // The Desktop group sits above the light and dark palettes.
    let group = |cx: &mut VisualTestContext, key: &str| {
        drawn(cx, format!("settings-theme-group-{key}")).expect("a group title")
    };
    let desktop = group(cx, "desktop");
    let card = drawn(cx, "settings-theme-omarchy".into()).expect("the Omarchy card");
    let light = group(cx, "light");
    assert!(desktop.bottom() <= card.top() && card.bottom() <= light.top());
    assert!(light.bottom() <= group(cx, "dark").top());
    assert!(
        cx.read(|cx| app.read(cx).card_names.borrow().clone())
            .contains(&("settings-theme-omarchy".into(), "Omarchy theme".into()))
    );
    let caption = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            let app = app.read(cx);
            let key = app.theme_card_body(omarchy).read(cx).key.clone().unwrap();
            (
                key.name.to_string(),
                key.description.to_string(),
                key.palette,
            )
        })
    };
    let tag = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            let app = app.read(cx);
            let key = app.theme_card_body(omarchy).read(cx).key.clone().unwrap();
            key.tag.map(|tag| tag.to_string())
        })
    };
    // The card's accessible name, with its state while unavailable.
    let named = |cx: &mut VisualTestContext| {
        page_shows(cx, "settings-theme-omarchy");
        cx.read(|cx| {
            app.read(cx)
                .card_names
                .borrow()
                .iter()
                .find(|(selector, _)| selector == "settings-theme-omarchy")
                .map(|(_, name)| name.clone())
        })
    };
    assert_eq!(
        caption(cx),
        ("Omarchy".into(), "Follows Tokyo Night".into(), mapped)
    );
    assert_eq!(tag(cx), None);
    assert_eq!(named(cx).as_deref(), Some("Omarchy theme"));
    assert!(!page_shows(cx, "settings-theme-omarchy-reason"));
    assert_eq!(
        cx.read(|cx| app.read(cx).theme_preview_body(omarchy).read(cx).palette()),
        mapped
    );

    let follow = |cx: &mut VisualTestContext| {
        page_shows(cx, "follow-system-switch");
        let row = cx.read(|cx| app.read(cx).follow_system_row.borrow().clone().unwrap());
        (row.0, row.1, row.2.to_string())
    };
    let toggle_follow = |cx: &mut VisualTestContext| {
        let switch = shown(cx, "follow-system-switch").expect("the Follow system switch");
        cx.simulate_click(switch.center(), Modifiers::default());
        settle(cx);
    };
    let choose = |cx: &mut VisualTestContext, selector: &str| {
        let card = shown(cx, selector.to_owned().leak()).expect("a picker card");
        cx.simulate_click(card.center(), Modifiers::default());
        settle(cx);
    };
    let unlocked = "Braden in Light Mode; your selected dark palette in Dark Mode.";
    toggle_follow(cx);
    assert!(cx.read(|cx| app.read(cx).settings.follow_system));
    assert_eq!(follow(cx), (true, false, unlocked.into()));

    choose(cx, "settings-theme-omarchy");
    let state = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            let app = app.read(cx);
            (
                app.settings.theme,
                app.settings.follow_system,
                app.selected_theme_card(cx),
            )
        })
    };
    assert_eq!(state(cx), (omarchy, false, Some(omarchy)));
    assert_eq!(cx.read(palette), mapped);
    let locked = (false, true, "Omarchy follows your desktop theme".to_owned());
    assert_eq!(follow(cx), locked);
    let miniature = miniature(cx, &app, omarchy);
    assert!(drawn(cx, format!("theme-check-{miniature}")).is_some());
    // The disabled switch does nothing.
    toggle_follow(cx);
    assert_eq!(state(cx), (omarchy, false, Some(omarchy)));
    assert_eq!(follow(cx), locked);

    // Selected, the card stays when the file goes, keeping the last palette.
    // Its caption says so in words that fit the card, the reason is a
    // line of its own under the card's row, and the accessible name
    // carries both.
    std::fs::remove_file(root.join("theme/colors.toml")).unwrap();
    cx.update(|_, cx| appearance::omarchy::refresh(cx));
    settle(cx);
    let card = drawn(cx, "settings-theme-omarchy".into()).expect("the card stays");
    assert_eq!(caption(cx).1, "Keeping Tokyo Night");
    assert_eq!(tag(cx).as_deref(), Some("Unavailable"));
    assert_eq!(
        named(cx).as_deref(),
        Some("Omarchy theme, unavailable: colors.toml is missing. Keeping Tokyo Night.")
    );
    let reason = drawn(cx, "settings-theme-omarchy-reason".into()).expect("the reason");
    assert!(card.bottom() <= reason.top() && reason.bottom() <= group(cx, "light").top());
    assert_eq!(
        cx.read(|cx| appearance::omarchy::card(cx).unwrap().reason),
        Some("colors.toml is missing.".into())
    );
    assert_eq!(cx.read(palette), mapped);
    let kept = caption(cx).1;
    assert!(appearance::omarchy::every_description("Tokyo Night").contains(&kept));
    omarchy_captions_fit(cx, &app);

    // Another card releases Follow system, and the unavailable card goes.
    choose(
        cx,
        &format!("settings-theme-{}", ThemeChoice::Nord as usize),
    );
    assert_eq!(
        state(cx),
        (
            ThemeSelection::BuiltIn(ThemeChoice::Nord),
            false,
            Some(ThemeSelection::BuiltIn(ThemeChoice::Nord))
        )
    );
    assert_eq!(follow(cx), (false, false, unlocked.into()));
    assert!(!page_shows(cx, "settings-theme-omarchy"));
    assert!(!page_shows(cx, "settings-theme-group-desktop"));
    toggle_follow(cx);
    assert!(cx.read(|cx| app.read(cx).settings.follow_system));
}

/// A disabled Switch fades its thumb with its track. The Follow system
/// switch, which the selected Omarchy theme disables, paints its track and
/// its thumb (`switch_thumb`, the palette's text) at half strength; enabled
/// with Midnight selected, off and on, it paints both at full strength, the
/// checked thumb in the accent's label colour.
#[cfg(target_os = "linux")]
#[gpui::test]
fn a_disabled_switch_fades_its_thumb_with_its_track(cx: &mut TestAppContext) {
    /// The fills of the Follow system switch in a frame that builds every
    /// view: its track, then the thumb inside it, its only filled quads.
    fn painted_switch(cx: &mut VisualTestContext) -> (Background, Background) {
        assert!(page_shows(cx, "follow-system-switch"));
        let switch = cx
            .debug_bounds("follow-system-switch")
            .expect("the Follow system switch");
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let device = px(1. / scale);
            let logical = |value: ScaledPixels| px(value.as_f32() / scale);
            let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
                inner.left() >= outer.left() - device
                    && inner.top() >= outer.top() - device
                    && inner.right() <= outer.right() + device
                    && inner.bottom() <= outer.bottom() + device
            };
            let fills = window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .map(|quad| {
                    let drawn = Bounds::from_corners(
                        point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                        point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                    );
                    (drawn, quad.background)
                })
                .filter(|(drawn, _)| inside(*drawn, switch))
                .collect::<Vec<_>>();
            let [(track, track_fill), (thumb, thumb_fill)] = fills[..] else {
                panic!("the switch paints a track and a thumb: {fills:?}");
            };
            assert!(inside(thumb, track), "the thumb lies on the track");
            assert!(thumb.size.width < track.size.width);
            (track_fill, thumb_fill)
        })
    }

    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("current");
    std::fs::create_dir_all(root.join("theme")).unwrap();
    let colors = include_bytes!("../tests/fixtures/omarchy/tokyo-night.toml");
    std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
    std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
    cx.update(|cx| drop(appearance::omarchy::install(Some(root.clone()), cx)));
    let (app, cx) = open_app(cx);
    // Selects a theme and Follow system, and returns what the switch
    // painted with whether it is on and whether it is locked.
    let select = |cx: &mut VisualTestContext, theme: ThemeSelection, follow_system: bool| {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.theme = theme;
                app.settings.follow_system = follow_system;
                app.apply_appearance(window, cx);
            })
        });
        settle(cx);
        let painted = painted_switch(cx);
        let row = cx.read(|cx| app.read(cx).follow_system_row.borrow().clone().unwrap());
        (painted, (row.0, row.1))
    };
    // The palette's track and thumb fills, faded by the track's 0.5 while
    // disabled.
    let expected = |cx: &mut VisualTestContext, checked: bool, disabled: bool| {
        let p = cx.read(palette);
        let fill = |color: u32| {
            let fill = Background::from(Hsla::from(rgb(color)));
            if disabled { fill.opacity(0.5) } else { fill }
        };
        (
            fill(if checked { p.accent } else { p.border }),
            fill(if checked { p.accent_foreground } else { p.text }),
        )
    };

    let midnight = ThemeSelection::BuiltIn(ThemeChoice::Midnight);
    for on in [false, true] {
        let (painted, state) = select(cx, midnight, on);
        assert_eq!(state, (on, false), "Midnight leaves the switch enabled");
        assert_eq!(painted, expected(cx, on, false), "enabled, on: {on}");
    }

    let (painted, state) = select(cx, ThemeSelection::Omarchy, false);
    assert_eq!(
        state,
        (false, true),
        "the Omarchy theme disables the switch"
    );
    assert_eq!(
        cx.read(palette),
        cx.read(|cx| appearance::omarchy::card(cx).unwrap().palette),
        "the Omarchy theme applies its mapped palette"
    );
    assert_eq!(painted, expected(cx, false, true), "disabled");
}

/// Every quad the last frame painted, in logical pixels, in paint order.
fn painted_quads(cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    cx.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .into_iter()
            .map(|quad| {
                let b = quad.bounds;
                Bounds::from_corners(
                    point(px(b.left().as_f32() / scale), px(b.top().as_f32() / scale)),
                    point(
                        px(b.right().as_f32() / scale),
                        px(b.bottom().as_f32() / scale),
                    ),
                )
            })
            .collect()
    })
}

/// Settings' Save editor keeps its shape while there is nothing to save,
/// as the default branch's Save does: it was the transparent helper
/// stretched to the card's width, so disabled it drew only a centred dim
/// label (x 1954–2605 at 2x in a 1480 px window). In a dark and a light
/// palette it now sits beside its field at the card's trailing edge, as
/// wide as its content, and paints Save's fill at Save's height.
#[gpui::test]
fn the_disabled_save_editor_keeps_its_shape(cx: &mut TestAppContext) {
    let (_, cx) = open_app(cx);
    let fills = |cx: &mut VisualTestContext, at: Bounds<Pixels>| {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let near = |device: gpui_kit::ScaledPixels, logical: Pixels| {
                (device.as_f32() - logical.as_f32() * scale).abs() < 0.5
            };
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    !quad.background.is_transparent()
                        && near(quad.bounds.left(), at.left())
                        && near(quad.bounds.top(), at.top())
                        && near(quad.bounds.right(), at.right())
                        && near(quad.bounds.bottom(), at.bottom())
                })
                .map(|quad| quad.background)
                .collect::<Vec<_>>()
        })
    };
    for theme in [ThemeChoice::Midnight, ThemeChoice::Daylight] {
        cx.update(|window, cx| theme.apply(Some(window), cx));
        // A frame that builds the whole page in the palette, read before
        // anything else draws.
        assert!(page_shows(cx, "save-external-editor"), "{theme:?}");
        let editor = cx.debug_bounds("save-external-editor").unwrap();
        let field = cx.debug_bounds("settings-editor-field").unwrap();
        let save = cx.debug_bounds("save-default-branch").unwrap();
        let (editor_fills, save_fills) = (fills(cx, editor), fills(cx, save));
        assert!(
            editor.left() >= field.right() && editor.right() == save.right(),
            "{theme:?}: Save editor {editor:?} beside its field {field:?}, ending with Save {save:?}"
        );
        assert_eq!(editor.size.height, save.size.height, "{theme:?}");
        assert!(
            !save_fills.is_empty() && editor_fills == save_fills,
            "{theme:?}: Save editor paints {editor_fills:?}, Save {save_fills:?}"
        );
    }
}

/// Toggling a Settings Switch moves nothing: every Switch track and the
/// rows around them keep their bounds, at 0.875, 1 and 1.25 of
/// the desktop text scale (Linux) and at 18 pt, and through the code font
/// row's off, pending, found and unavailable descriptions, which wrap to
/// different line counts in a 1,000 px window, with each toggle's save
/// answered before the page is measured. Show the project list is left
/// out: narrowing the page for its pane is what it does.
#[gpui::test]
fn toggling_a_settings_switch_moves_nothing(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    let sizes: &[(u8, f32)] = if cfg!(target_os = "linux") {
        &[(13, 0.875), (13, 1.0), (13, 1.25), (18, 1.0)]
    } else {
        &[(13, 1.0), (18, 1.0)]
    };
    let mut switches = vec!["follow-system-switch", "settings-reopen-last-row"];
    let mut rows = vec![
        "follow-system-switch",
        "custom-themes-new",
        "text-size-setting-interface",
        "text-size-setting-code",
        "settings-density-0",
        "settings-reopen-last-row",
        "settings-scroll",
    ];
    if cfg!(target_os = "linux") {
        switches.push("code-font-switch");
        rows.extend(["code-font-setting", "code-font-switch"]);
    }
    // Every Switch track, the toggled one's included, then each row.
    let measure = |cx: &mut VisualTestContext| {
        let mut bounds: Vec<_> = painted_quads(cx)
            .into_iter()
            .filter(|quad| quad.size == size(px(36.), px(20.)))
            .collect();
        // A checked track may paint in another order.
        bounds.sort_by(|a, b| {
            (a.top(), a.left())
                .partial_cmp(&(b.top(), b.left()))
                .unwrap()
        });
        for row in &rows {
            bounds.push(shown(cx, row).unwrap_or_else(|| panic!("{row}")));
        }
        bounds
    };
    for &(interface, desktop) in sizes {
        cx.simulate_resize(size(px(1000.), px(5000.)));
        cx.update(|window, cx| {
            #[cfg(target_os = "linux")]
            appearance::set_desktop_text_scale(desktop, cx);
            #[cfg(not(target_os = "linux"))]
            assert_eq!(desktop, 1.0, "only Linux reports a desktop text scale");
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                app.apply_appearance(window, cx);
                cx.notify();
            });
        });
        settle(cx);
        let at = format!("{interface} pt at {desktop}");
        let rest = painted_quads(cx);
        for selector in &switches {
            let holder = shown(cx, selector).unwrap_or_else(|| panic!("{selector}"));
            let track = rest
                .iter()
                .copied()
                .find(|quad| quad.size == size(px(36.), px(20.)) && holder.contains(&quad.center()))
                .unwrap_or_else(|| panic!("{at}: {selector} draws its track"));
            let expected = measure(cx);
            let assert_still = |cx: &mut VisualTestContext, state: &str| {
                let now = measure(cx);
                let moved: Vec<_> = expected
                    .iter()
                    .zip(&now)
                    .filter(|(before, after)| before != after)
                    .take(3)
                    .collect();
                assert!(
                    moved.is_empty() && now.len() == expected.len(),
                    "{at}: {state} moved {moved:?} ({} bounds, then {})",
                    expected.len(),
                    now.len()
                );
            };
            // A click saves the settings on the preference writer, a
            // thread `run_until_parked` does not wait for. Drained, each
            // save answers before the page is measured; undrained, a
            // loaded host's writer fell a queue's length behind the
            // clicks, its bounded queue refused the next save, and the
            // refusal's "Could not save settings" notice stood above
            // every switch (hosted run 42902520118).
            let toggle = |cx: &mut VisualTestContext| {
                cx.simulate_click(track.center(), Modifiers::default());
                cx.read(|cx| app.read(cx).preferences_writer.drain());
                settle(cx);
            };
            toggle(cx);
            assert_still(cx, &format!("turning {selector} on"));
            #[cfg(target_os = "linux")]
            if *selector == "code-font-switch" {
                use crate::desktop_text;
                let answer = |cx: &mut VisualTestContext, found: Result<SharedString, String>| {
                    cx.update(|_, cx| {
                        let lookup = desktop_text::code_font_lookup_generation(cx);
                        desktop_text::answer_code_font_lookup(lookup, found, cx)
                    });
                    settle(cx);
                };
                answer(cx, Ok("Desktop Mono".into()));
                assert_still(cx, "the desktop's font found");
                toggle(cx);
                assert_still(cx, "turning the desktop's font off");
                toggle(cx);
                answer(cx, Err("Proportional Sans is not monospace.".into()));
                assert_still(cx, "the desktop's font unusable");
            }
            toggle(cx);
            assert_still(cx, &format!("turning {selector} off again"));
        }
    }
}

/// The four switches' rows, for tests: each switch's ID, its row's
/// title, its description at rest and the selector drawn around the row.
fn switch_rows() -> Vec<(&'static str, &'static str, String, &'static str)> {
    #[cfg_attr(not(target_os = "linux"), expect(unused_mut))]
    let mut rows = vec![
        (
            "follow-system",
            FOLLOW_SYSTEM,
            "Braden in Light Mode; your selected dark palette in Dark Mode.".to_owned(),
            "follow-system-switch",
        ),
        (
            "settings-reopen-last",
            REOPEN_LAST,
            "Reopen the selected tab and retain up to eight saved repository tabs.".to_owned(),
            "settings-reopen-last-row",
        ),
        (
            "settings-project-pane",
            PROJECT_PANE,
            "Keep a pane on the far left to jump between known projects and sort them into groups."
                .to_owned(),
            "settings-project-pane-row",
        ),
    ];
    #[cfg(target_os = "linux")]
    rows.push((
        "settings-system-code-font",
        "Use the desktop's monospace font",
        format!(
            "Code uses the bundled {}. Turn on to use fontconfig's monospace font when it is fixed-width.",
            crate::mono()
        ),
        "code-font-switch",
    ));
    rows
}

/// The Switch track the last frame painted inside `row`.
fn track_in(cx: &mut VisualTestContext, row: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
    painted_quads(cx)
        .into_iter()
        .find(|quad| quad.size == size(px(36.), px(20.)) && row.contains(&quad.center()))
}

/// Each switch's row says its setting once, at 13 and 18 pt in a 461
/// and a 1,480 px window: the switch is named by the row's title and
/// described by its description, the row's text is not read apart from
/// the switch, the title stays inside the window, Follow system's on one
/// line, and the track ends the row, with no label drawn after it.
/// Follow system appearance drew its words beside its track as well as
/// in its title, which at 18 pt in a 461 px window wrapped to three lines
/// beside them, and each row's text read its title before its
/// description, the switch's name again, apart from the switch.
#[gpui::test]
fn each_switch_row_says_its_setting_once(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    let rows = switch_rows();
    for interface in [13, 18] {
        for width in [461., 1480.] {
            let at = format!("{interface} pt at {width} px");
            // Tall enough to paint every row of the narrow page.
            cx.simulate_resize(size(px(width), px(9000.)));
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.settings.interface_text_size = interface;
                    appearance::apply_text_sizes(interface, code, window, cx);
                    cx.notify();
                })
            });
            settle(cx);
            SWITCHES.with(|switches| switches.borrow_mut().clear());
            crate::native_accessibility::written::forget();
            assert!(page_shows(cx, "follow-system-switch"), "{at}");
            // The rows' text is read nowhere apart from the switches,
            // while List density's title and description are.
            let read = crate::native_accessibility::written::labels();
            assert!(
                read.iter().any(|(_, label)| label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("List density. "))),
                "{at}: List density is read"
            );
            for (_, title, description, _) in &rows {
                assert!(
                    !read
                        .iter()
                        .any(|(_, label)| label
                            .as_deref()
                            .is_some_and(|label| label.contains(title)
                                || label.contains(description.as_str()))),
                    "{at}: {title}'s row is read apart from its switch: {read:?}"
                );
            }
            // One line of a title, as a short one draws it.
            let line = drawn(cx, "setting-title: List density".into())
                .expect("List density's title")
                .size
                .height;
            for (id, title, description, row) in &rows {
                let built = switch_built(id);
                assert!(
                    built.as_ref().is_some_and(
                        |(name, built)| name.starts_with(title) && built == description
                    ),
                    "{at}: {id} is built {built:?}, not named by {title:?} and described \
                     by {description:?}"
                );
                for read in [format!("{title}. {description}"), description.clone()] {
                    assert!(
                        drawn(cx, format!("setting-description: {read}")).is_none(),
                        "{at}: {title}'s row reads {read:?} apart from its switch"
                    );
                }
                let drawn_title = drawn(cx, format!("setting-title: {title}"))
                    .unwrap_or_else(|| panic!("{at}: {title} is drawn"));
                assert!(
                    drawn_title.right() <= px(width),
                    "{at}: {title} {drawn_title:?} runs past the window"
                );
                // A longer title may wrap inside its room beside the
                // track ("Restore repository tabs on startup" takes two
                // lines at 18 pt in 461 px); Follow system's never does.
                if *title == FOLLOW_SYSTEM {
                    assert_eq!(
                        drawn_title.size.height, line,
                        "{at}: {title} {drawn_title:?} takes one line"
                    );
                }
                let row = shown(cx, row).unwrap_or_else(|| panic!("{at}: {row}"));
                let track = track_in(cx, row)
                    .unwrap_or_else(|| panic!("{at}: {title}'s row {row:?} draws its track"));
                assert!(
                    drawn_title.right() <= track.left() && track.right() == row.right(),
                    "{at}: {title}'s track {track:?} does not end its row {row:?} after its \
                     title {drawn_title:?}"
                );
            }
            // Each switch's own node, as the page's arguments build it.
            for (id, title, description, _) in &rows {
                let (name, _) = switch_built(id).expect("built");
                let node = switch_node(cx, id, title, name, description);
                assert_eq!(
                    (node.role(), node.label(), node.description()),
                    (
                        gpui::accesskit::Role::Switch,
                        Some(name),
                        Some(description.as_str())
                    ),
                    "{at}: {id}'s node"
                );
            }
        }
    }
}

/// The accessibility node of the switch [`switch_row`] builds for these
/// arguments, its base Switch built inside a draw, as
/// `views::tests::input_node` builds a field's.
fn switch_node(
    cx: &mut VisualTestContext,
    id: &'static str,
    title: &'static str,
    name: &'static str,
    description: &str,
) -> gpui::accesskit::Node {
    let description = SharedString::from(description.to_owned());
    crate::views::tests::view_node(cx, move |window, cx| {
        use gpui::Element as _;
        let base = switch_row(id, title, name, description.clone(), &[], cx).base(window, cx);
        let element = RenderOnce::render(base, window, cx).into_element();
        let mut node = gpui::accesskit::Node::new(element.a11y_role().expect("the switch's role"));
        element.write_a11y_info(&mut node);
        node
    })
}

/// Every Settings row's trailing control is centred on its title's first
/// line box, beside a description of one line or several, at 13 and 18 pt
/// in a 1,480 px and a 900 px window: List density's, Reduce motion's and
/// Increase contrast's segments, the two text sizes' controls, Graph lane
/// spacing's steppers, Your themes' actions and each switch's track, unless
/// its focus ring would then reach past its row's room above the title,
/// where it is lowered to that room. Only a switch's track at 13 pt is: a
/// switch fills its row, which the page reveals with a ring's room around
/// it, so the 20 px track, taller than the title's line (19.5 px on a 2x
/// display), keeps its top on the row's rather than rising 0.25 px above
/// it. Each lies within
/// half a device pixel of that place, the slot as tall as the title's
/// line as GPUI lays it out. At
/// 1,480 px and 13 pt the motion and contrast rows' descriptions, kept at
/// their tallest, and Restore repository tabs on startup's wrap to two
/// lines or more beside their controls; centred on the row, those
/// controls sat lower than List density's and drifted down with every
/// line a description wrapped to, and with their tops on the title's line
/// a segment group sat 4 px below the title's baseline at 18 pt and a
/// track 3.5 px above its cap centre. A control stacked below its text, as
/// in the narrow page, is left to the stacked layout.
#[gpui::test]
fn each_settings_row_centres_its_control_on_its_title_line(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    let ring = cx.read(appearance::button_ring_room);
    // Each control, the selector drawn around it, and the room above its
    // title in rems: a rem, or the 0.75 rem gap above Graph lane spacing;
    // a switch's is its ring's own room.
    let mut controls = vec![("List density", "settings-density-segments".to_owned(), 1.)];
    for setting in DisplaySetting::ALL {
        controls.push((setting.title(), format!("{}-segments", setting.key()), 1.));
    }
    controls.push((
        "Interface text size",
        "text-size-controls-interface".into(),
        1.,
    ));
    controls.push(("Code text size", "text-size-controls-code".into(), 1.));
    controls.push(("Graph lane spacing", "graph-spacing-controls".into(), 0.75));
    controls.push(("Your themes", "custom-themes-actions".into(), 1.));
    let switches = switch_rows();
    // Half a device pixel: layout puts every edge on the device grid, so
    // a control whose height differs from the line's by an odd number of
    // device pixels, as the 28 px steppers beside a 19.5 px line do, lies
    // a quarter pixel from exact centre at 2x; the rest are exact.
    let slack = cx.update(|window, _| px(0.5 / window.scale_factor()));
    let mut failures = Vec::new();
    let mut wrapped = Vec::new();
    let mut checked = 0;
    for (interface, width) in [(13, 1480.), (18, 1480.), (13, 900.), (18, 900.)] {
        let at = format!("{interface} pt at {width} px");
        // Tall enough to paint every row of the page.
        cx.simulate_resize(size(px(width), px(9000.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        settle(cx);
        let detail_line = appearance::ui_text(11.) * 1.5;
        // One line of a title, as a short one draws it.
        let line = drawn(cx, "setting-title: List density".into())
            .expect("List density's title")
            .size
            .height;
        let slot = cx.update(|window, _| title_line(window));
        assert_eq!(line, slot, "{at}: a title's line and its slot");
        let mut rows: Vec<(&str, Option<Bounds<Pixels>>, Pixels)> = controls
            .iter()
            .map(|(title, selector, room)| (*title, drawn(cx, selector.clone()), rem() * *room))
            .collect();
        for (_, title, _, row) in &switches {
            let row = drawn(cx, (*row).to_owned()).expect("the switch's row");
            rows.push((title, track_in(cx, row), ring));
        }
        for (title, control, room) in rows {
            let heading = drawn(cx, format!("setting-title: {title}"));
            let detail = drawn(cx, format!("setting-detail: {title}"));
            let (Some(control), Some(heading), Some(detail)) = (control, heading, detail) else {
                failures.push(format!("{at}: {title}'s row is not drawn whole"));
                continue;
            };
            if control.top() + slack >= detail.bottom() {
                // Stacked below its text.
                continue;
            }
            checked += 1;
            if detail.size.height > detail_line * 1.5 {
                wrapped.push(format!("{title} at {at}"));
            }
            let centred = heading.top() + line / 2. - control.size.height / 2.;
            let lowest = heading.top() - (room - ring);
            let expected = centred.max(lowest);
            if (control.top() - expected).abs() > slack {
                failures.push(format!(
                    "{at}: {title}'s control {:?} starts at {:?}, not {expected:?}, \
                     centred on its title's first line from {:?}, {line:?} tall, its \
                     description {:?} tall",
                    control.size,
                    control.top(),
                    heading.top(),
                    detail.size.height
                ));
            }
        }
    }
    // Every control is beside its text at least at 1,480 px.
    assert!(
        checked >= 2 * (controls.len() + switches.len()),
        "{checked} controls checked beside their text"
    );
    for title in [
        DisplaySetting::ReduceMotion.title(),
        DisplaySetting::IncreaseContrast.title(),
        REOPEN_LAST,
    ] {
        let beside = format!("{title} at 13 pt at 1480 px");
        assert!(
            wrapped.contains(&beside),
            "{title}'s description wraps beside its control at 13 pt in 1,480 px: \
             {wrapped:?}"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Where the leftmost text run the last frame drew inside `area` starts,
/// in a window whose root marks every text run
/// (`width_walk::text_mark`).
fn text_start_in(cx: &mut VisualTestContext, area: Bounds<Pixels>) -> Option<Pixels> {
    let mark = gpui_kit::Background::from(crate::width_walk::text_mark());
    cx.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background == mark)
            .map(|quad| {
                let b = quad.bounds;
                Bounds::from_corners(
                    point(px(b.left().as_f32() / scale), px(b.top().as_f32() / scale)),
                    point(
                        px(b.right().as_f32() / scale),
                        px(b.bottom().as_f32() / scale),
                    ),
                )
            })
            .filter(|run| area.contains(&run.center()))
            .map(|run| run.left())
            .reduce(Pixels::min)
    })
}

/// The focus ring the last frame drew around a Button's or a Switch's
/// `control`: the bordered quad the installed `Theme::button_focus_ring`
/// paints there, within a device pixel.
fn ring_around(cx: &mut VisualTestContext, control: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
    cx.update(|window, cx| {
        let scale = window.scale_factor();
        let setting = gpui_kit::component::Theme::global(cx).button_focus_ring;
        let footprint = control.dilate(setting.gap + setting.width);
        window.painted_quads().into_iter().find_map(|quad| {
            let b = quad.bounds;
            let drawn = Bounds::from_corners(
                point(px(b.left().as_f32() / scale), px(b.top().as_f32() / scale)),
                point(
                    px(b.right().as_f32() / scale),
                    px(b.bottom().as_f32() / scale),
                ),
            );
            let near = [
                (drawn.left(), footprint.left()),
                (drawn.top(), footprint.top()),
                (drawn.right(), footprint.right()),
                (drawn.bottom(), footprint.bottom()),
            ]
            .into_iter()
            .all(|(edge, expected)| (edge - expected).abs() <= px(1. / scale));
            (near && quad.border_widths.top.as_f32() == setting.width.as_f32() * scale)
                .then_some(drawn)
        })
    })
}

/// Centred on its title's first line, a focused control's ring stays
/// inside its row: each segment's below its row's separator at 13 and
/// 18 pt (10 and 15 px below it, the group reaching 3 and 4.5 px above
/// its title), and each switch's inside the row the switch fills at
/// 18 pt, where the track sits 3.5 px below its title's top. At 13 pt,
/// where the 20 px track is taller than the title's line (19.5 px on a 2x
/// display), the ring keeps to the ring's room the page reveals the row
/// with: centred, the track rose 0.25 px above the row, and the page's
/// edge cut its ring with the row revealed at the top of the view.
#[gpui::test]
fn a_centred_control_keeps_its_focus_ring_inside_its_row(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    let focus = |cx: &mut VisualTestContext, id: ElementId| {
        let handle = cx.update(|_, cx| app.update(cx, |app, cx| app.page_focus(id, cx)));
        cx.update(|window, cx| window.focus(&handle, cx));
        settle(cx);
    };
    let mut segments: Vec<(String, String, ElementId)> = vec![(
        "settings-density-0".into(),
        "settings-density-row".into(),
        ("settings-density", 0usize).into(),
    )];
    for setting in DisplaySetting::ALL {
        segments.push((
            format!("{}-0", setting.key()),
            format!("{}-row", setting.key()),
            (setting.key(), 0usize).into(),
        ));
    }
    for interface in [13, 18] {
        cx.simulate_resize(size(px(1480.), px(9000.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        settle(cx);
        for (segment, row, id) in &segments {
            focus(cx, id.clone());
            let control = drawn(cx, segment.clone()).expect("the segment");
            let row = drawn(cx, row.clone()).expect("the segment's row");
            let ring = ring_around(cx, control)
                .unwrap_or_else(|| panic!("{interface} pt: {segment} draws its ring"));
            // The separator is the row's 1 px top border.
            assert!(
                ring.top() >= row.top() + px(1.) && ring.bottom() <= row.bottom(),
                "{interface} pt: {segment}'s ring {ring:?} leaves its row {row:?}"
            );
        }
        let room = cx.read(appearance::button_ring_room);
        for (id, title, _, row) in switch_rows() {
            focus(cx, id.into());
            let row = drawn(cx, row.to_owned()).expect("the switch's row");
            let track = track_in(cx, row).expect("the track");
            let ring = ring_around(cx, track)
                .unwrap_or_else(|| panic!("{interface} pt: {title}'s switch draws its ring"));
            let top = if interface == 18 {
                row.top()
            } else {
                row.top() - room
            };
            assert!(
                ring.top() >= top && ring.bottom() <= row.bottom(),
                "{interface} pt: {title}'s ring {ring:?} leaves its row {row:?}"
            );
        }
    }
}

/// A control too tall to centre on its title's first line without its
/// focus ring passing the row's room above the title is lowered just
/// enough to keep the ring inside: the ring then meets the separator's
/// lower edge. Shorter ones are centred, whether shorter than the line or
/// reaching above the title. No Settings control is that tall, so a
/// synthetic one stands in, in a row built as a separated row is: a 1 px
/// separator above a rem of padding.
#[gpui::test]
fn a_control_too_tall_to_centre_is_lowered_to_keep_its_ring_in_its_row(cx: &mut TestAppContext) {
    struct Row {
        height: Pixels,
    }
    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().pt(px(100.)).child(
                div()
                    .debug_selector(|| "row".into())
                    .flex()
                    .items_start()
                    .justify_between()
                    .pt(rem())
                    .border_t_1()
                    .child(
                        div()
                            .debug_selector(|| "title".into())
                            .text_size(appearance::ui_text(12.))
                            .line_height(relative(TITLE_LINE_HEIGHT))
                            .child("Title"),
                    )
                    .child(TitleLine::new(
                        div()
                            .debug_selector(|| "control".into())
                            .w(px(40.))
                            .h(self.height)
                            .into_any_element(),
                        rem(),
                        px(3.),
                    )),
            )
        }
    }
    crate::views::tests::init_app(cx);
    let (view, cx) = cx.add_window_view(|_, _| Row { height: px(10.) });
    for interface in [13, 18] {
        cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
        for height in [10., 30., 40., 60., 120.] {
            cx.update(|window, cx| {
                view.update(cx, |row, cx| {
                    row.height = px(height);
                    cx.notify();
                });
                window.draw(cx).clear(cx);
            });
            let at = format!("{interface} pt, {height} px tall");
            let row = cx.debug_bounds("row").expect("the row");
            let title = cx.debug_bounds("title").expect("the title");
            let control = cx.debug_bounds("control").expect("the control");
            let centre = title.top() + title.size.height / 2.;
            let ring = control.dilate(px(3.));
            let separator = row.top() + px(1.);
            let centred = (control.center().y - centre).abs() <= px(0.5);
            let lowered = (ring.top() - separator).abs() <= px(0.5);
            assert!(
                (centred && ring.top() + px(0.5) >= separator) || (lowered && !centred),
                "{at}: the control {control:?} is neither centred on {centre:?} with its \
                 ring {ring:?} inside the row nor lowered to put it on the separator \
                 {separator:?}"
            );
            // Room enough to centre a 30 px control at either size; a
            // 120 px one never fits.
            if height <= 30. {
                assert!(centred, "{at}: centred");
            }
            if height >= 120. {
                assert!(lowered, "{at}: lowered to the separator");
            }
        }
    }
}

/// Your themes' header wraps as the text sizes' rows do. In a 461 × 490
/// window at 18 pt, and at 13 pt in a 410 × 490 one, New theme… and
/// Import a theme file no longer fit beside the text with the text at
/// least 180 px (scaled) wide, so they start a line of their own at the
/// title's left edge (New theme…, a ghost Button, by its icon, its frame
/// its label inset before it, and a text size's small ghost − by its
/// label, its frame its 0.5 rem padding before it), a row gap (0.75 rem)
/// below the description, with
/// nothing reserved above them; the text takes the row's whole width,
/// nothing overlaps, the row ends at the actions' foot, and each action's
/// focus ring is drawn whole inside the card and the page's view. At
/// 13 pt in 461 px the header is 8 px short of holding both with the
/// labels Linux measures, so CoreText's may fit: there the test holds
/// whichever layout it takes to that layout's rule. 410 px leaves it
/// about 60 px short, so the wrap asserted there holds on every
/// platform. At 1,480 × 973 the actions stay beside the text, centred on
/// the title's first line. The text sizes'
/// controls take the same rule wherever they wrap, as at 18 pt in 461 px:
/// as tall as the title's line, they reserve nothing above themselves
/// there either. At 461 px and 18 pt the header kept its actions beside
/// a text column 66 px wide, its title on two lines and its description
/// a word a line, beside 230 px of empty card; placed in a title line's
/// slot on their own line, the actions started 4.25 px above the row gap
/// at 13 pt and 5.9 px above it at 18 pt.
#[gpui::test]
fn the_your_themes_header_wraps_its_actions_below_its_text(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    let ring = cx.read(appearance::button_ring_room);
    let slack = cx.update(|window, _| px(0.5 / window.scale_factor()));
    // Each row's title, its control, the row the control wraps in and
    // how far its leading ghost Button's content starts inside its
    // frame, which on a line of its own stands that far before the
    // title's edge: New theme…'s label inset, a text size's small −'s
    // 0.5 rem padding.
    let rows: [(_, _, _, fn() -> Pixels); 3] = [
        (
            "Your themes",
            "custom-themes-actions",
            "custom-themes-header",
            || appearance::ui_size(crate::LABEL_INSET),
        ),
        (
            "Interface text size",
            "text-size-controls-interface",
            "text-size-row-interface",
            || rem() * 0.5,
        ),
        (
            "Code text size",
            "text-size-controls-code",
            "text-size-row-code",
            || rem() * 0.5,
        ),
    ];
    let mut wrapped = Vec::new();
    let mut failures = Vec::new();
    for (interface, width, height) in [
        (13, 410., 490.),
        (13, 461., 490.),
        (18, 461., 490.),
        (13, 1480., 973.),
        (18, 1480., 973.),
    ] {
        let at = format!("{interface} pt at {width} × {height}");
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        settle(cx);
        let gap = rem() * 0.75;
        let line = cx.update(|window, _| title_line(window));
        for (title, control, row, lead) in rows {
            let inset = cx.update(|window, _| window.pixel_snap(lead()));
            let (Some(heading), Some(detail), Some(control), Some(row)) = (
                drawn(cx, format!("setting-title: {title}")),
                drawn(cx, format!("setting-detail: {title}")),
                drawn(cx, control.to_owned()),
                drawn(cx, row.to_owned()),
            ) else {
                failures.push(format!("{at}: {title}'s row is not drawn whole"));
                continue;
            };
            // The text's column, which the description fills.
            let text = detail.size.width;
            if text + slack < appearance::ui_size(180.) && (text - row.size.width).abs() > slack {
                failures.push(format!(
                    "{at}: {title}'s text is {text:?} wide in a {:?} row",
                    row.size.width
                ));
            }
            if control.top() + slack < detail.bottom() {
                // Beside the text, centred on the title's first line.
                let expected =
                    heading.top() + beside_title(line, control.size.height, rem() - ring);
                if control.left() + slack < detail.right() + gap
                    || (control.top() - expected).abs() > slack
                {
                    failures.push(format!(
                        "{at}: {title}'s control {control:?} beside its text {detail:?} \
                         does not start at {expected:?}, centred on its title's line"
                    ));
                }
                continue;
            }
            wrapped.push(format!("{title} at {at}"));
            let edge = heading.left() - inset;
            if (control.left() - edge).abs() > slack
                || (control.top() - detail.bottom() - gap).abs() > slack
                || (text - row.size.width).abs() > slack
                || (control.bottom() - row.bottom()).abs() > slack
            {
                failures.push(format!(
                    "{at}: {title}'s control {control:?} on its own line does not start \
                     at {edge:?}, by its title's left edge {:?}, {gap:?} below its \
                     description {detail:?}, ending its row {row:?}",
                    heading.left()
                ));
            }
        }
        if width > 1000. {
            continue;
        }
        // Each action's ring, focused from the keyboard, which reveals it.
        for action in ["custom-themes-new", "custom-themes-import"] {
            cx.simulate_keystrokes("f13");
            settle(cx);
            let focus = cx.update(|_, cx| app.update(cx, |app, cx| app.page_focus(action, cx)));
            cx.update(|window, cx| window.focus(&focus, cx));
            settle(cx);
            let (Some(button), Some(card), Some(view)) = (
                drawn(cx, action.to_owned()),
                drawn(cx, "custom-themes-card".to_owned()),
                drawn(cx, "settings-scroll".to_owned()),
            ) else {
                failures.push(format!("{at}: {action} is not drawn"));
                continue;
            };
            let Some(drawn_ring) = ring_around(cx, button) else {
                failures.push(format!("{at}: {action} draws no ring"));
                continue;
            };
            let inside = |outer: Bounds<Pixels>| {
                drawn_ring.left() + slack >= outer.left()
                    && drawn_ring.top() + slack >= outer.top()
                    && drawn_ring.right() <= outer.right() + slack
                    && drawn_ring.bottom() <= outer.bottom() + slack
            };
            if !inside(card) || !inside(view) {
                failures.push(format!(
                    "{at}: {action}'s ring {drawn_ring:?} leaves the card {card:?} or the \
                     page's view {view:?}"
                ));
            }
        }
    }
    for (size, width) in [(13, 410), (18, 461)] {
        let header = format!("Your themes at {size} pt at {width} × 490");
        assert!(
            wrapped.contains(&header),
            "the header wraps at {size} pt in {width} px: {wrapped:?}"
        );
    }
    assert!(
        wrapped.contains(&"Interface text size at 18 pt at 461 × 490".to_owned()),
        "the interface text size's controls wrap at 18 pt in 461 px: {wrapped:?}"
    );
    assert!(
        !wrapped.iter().any(|row| row.contains("1480")),
        "every control stays beside its text at 1,480 px: {wrapped:?}"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Where Your themes' actions wrap onto a line of their own, New theme…,
/// a ghost Button whose frame paints nothing at rest, stands its "+" icon
/// on the title's left edge, its frame the `button` helper's label inset
/// before it, and Import… follows at the actions' 0.5 rem gap (DESIGN.md,
/// "One left edge"). The icon draws no text, so the test reads where its
/// label is drawn, every text run marked as the layout grid marks them,
/// and places the icon its 16 px and their 6 px gap before it. Its hover
/// fill, and the ring Tab draws around it, stay inside the card's 1 px
/// border and the page's view: the ring's outer edge falls on the
/// border's inner edge at 13 pt, where the card's 1 rem padding less the
/// inset is the ring's 3 px, and a pixel inside it at 18 pt. In
/// 461 × 490 Linux wraps the actions at 13 and 18 pt;
/// CoreText's narrower labels may fit them beside the text at 13 pt, so
/// there the test holds whichever layout it takes to that layout's rule,
/// and 410 px, about 60 px short, wraps them on every platform. Beside
/// the text, at 1,480 × 973, the actions stay where they were: the
/// 0.75 rem gap past the text's column, ending the header, centred on the
/// title's first line. Standing its frame on the title's edge, New
/// theme… started its icon 10 px past the title at 13 pt and 14 px at
/// 18 pt.
#[gpui::test]
fn new_theme_stands_its_icon_on_the_titles_edge_where_the_actions_wrap(cx: &mut TestAppContext) {
    // Every text run marked, so the test reads where New theme…'s label
    // is drawn.
    let (app, cx) = open_styled_app(cx, |root| root.text_bg(crate::width_walk::text_mark()));
    let label_in = text_start_in;
    cx.update(|window, cx| {
        cx.set_reduce_motion(true);
        ThemeChoice::Midnight.apply(Some(window), cx);
    });
    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    let room = cx.read(appearance::button_ring_room);
    let new_theme =
        cx.update(|_, cx| app.update(cx, |app, cx| app.page_focus("custom-themes-new", cx)));
    let (scale, slack) = cx.update(|window, _| {
        let scale = window.scale_factor();
        (scale, px(0.5 / scale))
    });
    // The non-transparent fills the last frame painted at `at`.
    let fills_at = |cx: &mut VisualTestContext, at: Bounds<Pixels>| {
        cx.update(|window, _| {
            let near =
                |a: ScaledPixels, b: Pixels| (a.as_f32() / scale - f32::from(b)).abs() <= 0.5;
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    !quad.background.is_transparent()
                        && near(quad.bounds.left(), at.left())
                        && near(quad.bounds.top(), at.top())
                        && near(quad.bounds.right(), at.right())
                        && near(quad.bounds.bottom(), at.bottom())
                })
                .count()
        })
    };
    let mut wrapped = Vec::new();
    let mut failures = Vec::new();
    for (interface, width, height) in [
        (13, 461., 490.),
        (18, 461., 490.),
        (13, 410., 490.),
        (13, 1480., 973.),
        (18, 1480., 973.),
    ] {
        let at = format!("{interface} pt at {width} × {height}");
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            });
            window.blur(cx);
        });
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        settle(cx);
        let inset =
            cx.update(|window, _| window.pixel_snap(appearance::ui_size(crate::LABEL_INSET)));
        let (Some(title), Some(detail), Some(header), Some(actions), Some(new), Some(import)) = (
            drawn(cx, "setting-title: Your themes".to_owned()),
            drawn(cx, "setting-detail: Your themes".to_owned()),
            drawn(cx, "custom-themes-header".to_owned()),
            drawn(cx, "custom-themes-actions".to_owned()),
            drawn(cx, "custom-themes-new".to_owned()),
            drawn(cx, "custom-themes-import".to_owned()),
        ) else {
            failures.push(format!("{at}: Your themes' header is not drawn whole"));
            continue;
        };
        if actions.top() + slack < detail.bottom() {
            // Beside the text, where the actions always stood: the row's
            // gap past the text's column, which the description fills,
            // ending the header, centred on the title's first line.
            let line = cx.update(|window, _| title_line(window));
            let top = title.top() + beside_title(line, actions.size.height, rem() - room);
            if (actions.left() - detail.right() - rem() * 0.75).abs() > slack
                || (actions.right() - header.right()).abs() > slack
                || (actions.top() - top).abs() > slack
                || (new.left() - actions.left()).abs() > slack
            {
                failures.push(format!(
                    "{at}: beside the text {detail:?} the actions {actions:?} moved from \
                     0.75 rem past it, ending the header {header:?}, at {top:?}"
                ));
            }
            continue;
        }
        wrapped.push(format!("{interface} pt at {width}"));
        // Its frame its label inset before the title's edge, as layout
        // places the frame's padding on device pixels, and Import… at the
        // actions' gap.
        if (new.left() - (title.left() - inset)).abs() > slack
            || (import.left() - new.right() - rem() * 0.5).abs() > slack
        {
            failures.push(format!(
                "{at}: wrapped, New theme… {new:?} does not stand its frame {inset:?} \
                 before the title's edge {:?}, or Import… {import:?} is not 0.5 rem after \
                 it",
                title.left()
            ));
        }
        // Tab onto New theme…, from the control before it, which also
        // scrolls it into the page's view.
        cx.update(|window, cx| {
            window.focus(&new_theme, cx);
            window.focus_prev(cx);
        });
        settle(cx);
        cx.simulate_keystrokes("tab");
        settle(cx);
        if !cx.update(|window, _| new_theme.is_focused(window)) {
            failures.push(format!("{at}: Tab does not reach New theme…"));
            continue;
        }
        let (Some(new), Some(card), Some(view)) = (
            drawn(cx, "custom-themes-new".to_owned()),
            drawn(cx, "custom-themes-card".to_owned()),
            drawn(cx, "settings-scroll".to_owned()),
        ) else {
            failures.push(format!("{at}: focused, New theme… is not drawn"));
            continue;
        };
        // The icon, in view now: where the label New theme… draws
        // starts, less the icon and its gap as layout places them on
        // device pixels, on the title's edge. Below the view, the label
        // is not drawn.
        let (Some(title), Some(label)) = (
            drawn(cx, "setting-title: Your themes".to_owned()),
            label_in(cx, new),
        ) else {
            failures.push(format!("{at}: in view, New theme… {new:?} draws no label"));
            continue;
        };
        let icon = cx.update(|window, _| {
            label
                - window.pixel_snap(appearance::ui_size(6.))
                - window.pixel_snap(appearance::ui_size(16.))
        });
        if (icon - title.left()).abs() > slack {
            failures.push(format!(
                "{at}: wrapped, New theme… draws its label at {label:?}, its icon at \
                 {icon:?}, not on the title's edge {:?}",
                title.left()
            ));
        }
        let Some(ring) = ring_around(cx, new) else {
            failures.push(format!("{at}: focused by Tab, New theme… draws no ring"));
            continue;
        };
        // Inside the card's 1 px border, over no other control, and the
        // page's view.
        let within = |outer: Bounds<Pixels>| {
            ring.left() + slack >= outer.left()
                && ring.top() + slack >= outer.top()
                && ring.right() <= outer.right() + slack
                && ring.bottom() <= outer.bottom() + slack
        };
        let border = Bounds::from_corners(
            card.origin + point(px(1.), px(1.)),
            card.bottom_right() - point(px(1.), px(1.)),
        );
        if !within(border) || !within(view) {
            failures.push(format!(
                "{at}: New theme…'s ring {ring:?} leaves the card's border {border:?} or \
                 the page's view {view:?}"
            ));
        }
        // At rest the frame paints nothing: what a person sees is its
        // icon. Hovered, its fill stays inside the card's border.
        cx.update(|window, cx| window.blur(cx));
        settle(cx);
        if fills_at(cx, new) != 0 {
            failures.push(format!("{at}: New theme… paints a fill at rest"));
        }
        cx.simulate_mouse_move(new.center(), None, Modifiers::default());
        settle(cx);
        if fills_at(cx, new) == 0 || new.left() + slack < border.left() {
            failures.push(format!(
                "{at}: hovered, New theme… {new:?} paints no fill, or one past the card's \
                 border {border:?}"
            ));
        }
    }
    for required in ["18 pt at 461", "13 pt at 410"] {
        assert!(
            wrapped.iter().any(|layout| layout == required),
            "the actions wrap at {required} px: {wrapped:?}"
        );
    }
    assert!(
        !wrapped.iter().any(|layout| layout.ends_with("1480")),
        "the actions stay beside the text at 1,480 px: {wrapped:?}"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Where a ghost Button, which paints nothing at rest, stands at a
/// Settings card's text edge, its content stands on the edge and its
/// frame in the card's padding before it (DESIGN.md, "One left edge"),
/// at 13 and 18 pt: Git identity's Manage named profiles…, its "user"
/// icon on the edge its field labels and notes start at; Reset edits,
/// wrapped onto a line of its own below Save repository identity, its
/// icon there too; the Interface text size's −, wrapped below its
/// description, its label on its title's edge; and, with no repository
/// open, Choose a project, its icon on the edge the card's lines start
/// at. The icons draw no text, so the test reads where each label is
/// drawn, every text run marked, and places the icon its 16 px and their
/// 6 px gap before it. Tab onto each, where it stands at the edge, draws
/// its ring whole inside the card's 1 px border and the page's view.
/// Beside Save, in a single column of cards, Reset edits stays 0.5 rem
/// past it; beside its text the − is not measured, as it never moved
/// there. Standing their frames on the edge, Reset edits and Choose a
/// project started their icons 10 px past it at 13 pt and 14 px at
/// 18 pt, and the − its label 6.5 and 9 px; Manage, stretched across the
/// card, centred its content in it.
#[gpui::test]
async fn settings_ghost_buttons_stand_their_content_on_the_cards_text_edge(
    cx: &mut TestAppContext,
) {
    let (_fixture, app, cx, _) =
        crate::width_walk::open(crate::width_walk::Page::History, cx).await;
    cx.update(|window, cx| {
        cx.set_reduce_motion(true);
        app.update(cx, |app, cx| app.show_settings(window, cx))
    });
    crate::views::tests::settle(&app, cx).await;
    // An edit, so Save and Reset edits take focus.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.identity_name_for_test()
                .update(cx, |input, cx| input.set_value("Another Name", window, cx))
        })
    });
    settle(cx);
    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    let handles = cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            [
                app.page_focus("settings-manage-profiles", cx),
                app.page_focus("use-current-identity", cx),
                app.page_focus(("text-size-decrease", 0_usize), cx),
            ]
        })
    });
    let mut wrapped = Vec::new();
    let mut failures = Vec::new();
    for (interface, width) in [
        (13, 1480.),
        (18, 1480.),
        (13, 1000.),
        (13, 760.),
        (18, 1000.),
        (18, 760.),
        (13, 360.),
        (18, 461.),
    ] {
        let at = format!("{interface} pt at {width}");
        cx.simulate_resize(size(px(width), px(980.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        settle(cx);
        let slack = cx.update(|window, _| px(1. / window.scale_factor()));
        for (index, selector) in [
            "settings-manage-profiles",
            "use-current-identity",
            "text-size-decrease-interface",
        ]
        .into_iter()
        .enumerate()
        {
            // Tab onto it, from the control before it, which also scrolls
            // it into the page's view.
            cx.update(|window, cx| {
                window.focus(&handles[index], cx);
                window.focus_prev(cx);
            });
            settle(cx);
            cx.simulate_keystrokes("tab");
            settle(cx);
            if !cx.update(|window, _| handles[index].is_focused(window)) {
                failures.push(format!("{at}: Tab does not reach {selector}"));
                continue;
            }
            let card_selector = if index == 2 {
                "settings-section: Appearance"
            } else {
                "settings-section: Git identity"
            };
            let (Some(frame), Some(card), Some(view)) = (
                drawn(cx, selector.to_owned()),
                drawn(cx, card_selector.to_owned()),
                drawn(cx, "settings-scroll".to_owned()),
            ) else {
                failures.push(format!("{at}: {selector} is not drawn"));
                continue;
            };
            let Some(label) = text_start_in(cx, frame) else {
                failures.push(format!("{at}: {selector} {frame:?} draws no label"));
                continue;
            };
            // Where its content starts, and the text edge it stands at:
            // the card's text where its lines lead the card, the text
            // size's title for the −.
            let (content, edge) = match index {
                0 => (
                    icon_before(cx, label),
                    text_start_in(cx, card.intersect(&view)),
                ),
                1 => {
                    let Some(save) = drawn(cx, "save-repository-identity".to_owned()) else {
                        failures.push(format!("{at}: Save repository identity is not drawn"));
                        continue;
                    };
                    if save.top() + slack >= frame.top() {
                        // Beside Save, where it always stood.
                        wrapped.push(format!("Reset edits beside Save at {at}"));
                        if (frame.left() - save.right() - rem() * 0.5).abs() > slack {
                            failures.push(format!(
                                "{at}: beside Save {save:?}, Reset edits {frame:?} is not \
                                 0.5 rem past it"
                            ));
                        }
                        continue;
                    }
                    wrapped.push(format!("Reset edits at {at}"));
                    (
                        icon_before(cx, label),
                        text_start_in(cx, card.intersect(&view)),
                    )
                }
                _ => {
                    let (Some(title), Some(detail)) = (
                        drawn(cx, "setting-title: Interface text size".to_owned()),
                        drawn(cx, "setting-detail: Interface text size".to_owned()),
                    ) else {
                        failures.push(format!("{at}: the text size's row is not drawn"));
                        continue;
                    };
                    if frame.top() + slack < detail.bottom() {
                        // Beside its text, where it always stood.
                        continue;
                    }
                    wrapped.push(format!("− at {at}"));
                    (label, Some(title.left()))
                }
            };
            failures.extend(on_the_edge(
                cx, &at, selector, content, edge, frame, card, view,
            ));
        }
    }
    // Git identity with no repository open: Choose a project, its
    // "folder" icon on the edge the card's lines start at.
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.close_repository_tab(0, window, cx);
            app.show_settings(window, cx)
        })
    });
    crate::views::tests::settle(&app, cx).await;
    let choose =
        cx.update(|_, cx| app.update(cx, |app, cx| app.page_focus("settings-open-projects", cx)));
    for (interface, width) in [(13, 1480.), (18, 1480.), (13, 360.), (18, 461.)] {
        let at = format!("{interface} pt at {width}, no repository open");
        cx.simulate_resize(size(px(width), px(980.)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        settle(cx);
        cx.update(|window, cx| {
            window.focus(&choose, cx);
            window.focus_prev(cx);
        });
        settle(cx);
        cx.simulate_keystrokes("tab");
        settle(cx);
        if !cx.update(|window, _| choose.is_focused(window)) {
            failures.push(format!("{at}: Tab does not reach Choose a project"));
            continue;
        }
        let (Some(frame), Some(card), Some(view)) = (
            drawn(cx, "settings-open-projects".to_owned()),
            drawn(cx, "settings-section: Git identity".to_owned()),
            drawn(cx, "settings-scroll".to_owned()),
        ) else {
            failures.push(format!("{at}: Choose a project is not drawn"));
            continue;
        };
        let Some(label) = text_start_in(cx, frame) else {
            failures.push(format!("{at}: Choose a project {frame:?} draws no label"));
            continue;
        };
        let content = icon_before(cx, label);
        let edge = text_start_in(cx, card.intersect(&view));
        failures.extend(on_the_edge(
            cx,
            &at,
            "settings-open-projects",
            content,
            edge,
            frame,
            card,
            view,
        ));
    }
    for required in [
        "Reset edits at 13 pt at 360",
        "Reset edits at 18 pt at 461",
        "− at 13 pt at 360",
        "− at 18 pt at 461",
    ] {
        assert!(
            wrapped.iter().any(|layout| layout == required),
            "{required} wraps: {wrapped:?}"
        );
    }
    for size in ["13 pt", "18 pt"] {
        assert!(
            wrapped
                .iter()
                .any(|layout| layout.starts_with(&format!("Reset edits beside Save at {size}"))),
            "Reset edits stands beside Save in some layout at {size}: {wrapped:?}"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A leading icon's left edge, from where the label after it starts:
/// the icon's 16 px and their 6 px gap before it, as layout places them
/// on device pixels.
fn icon_before(cx: &mut VisualTestContext, label: Pixels) -> Pixels {
    cx.update(|window, _| {
        label
            - window.pixel_snap(appearance::ui_size(6.))
            - window.pixel_snap(appearance::ui_size(16.))
    })
}

/// Where a ghost Button `selector`, drawn at `frame` and focused by Tab,
/// fails to stand its `content` on the text's `edge`, within a device
/// pixel, or draws its ring past its `card`'s 1 px border or the page's
/// `view`, by half a device pixel.
#[expect(clippy::too_many_arguments)]
fn on_the_edge(
    cx: &mut VisualTestContext,
    at: &str,
    selector: &str,
    content: Pixels,
    edge: Option<Pixels>,
    frame: Bounds<Pixels>,
    card: Bounds<Pixels>,
    view: Bounds<Pixels>,
) -> Vec<String> {
    let slack = cx.update(|window, _| px(1. / window.scale_factor()));
    let Some(edge) = edge else {
        return vec![format!("{at}: no text is drawn at {selector}'s card")];
    };
    let mut failures = Vec::new();
    if (content - edge).abs() > slack {
        failures.push(format!(
            "{at}: {selector} {frame:?} starts its content at {content:?}, not on the \
             text's edge {edge:?}"
        ));
    }
    let Some(ring) = ring_around(cx, frame) else {
        failures.push(format!("{at}: focused by Tab, {selector} draws no ring"));
        return failures;
    };
    let within = |outer: Bounds<Pixels>| {
        ring.left() + slack / 2. >= outer.left()
            && ring.top() + slack / 2. >= outer.top()
            && ring.right() <= outer.right() + slack / 2.
            && ring.bottom() <= outer.bottom() + slack / 2.
    };
    let border = Bounds::from_corners(
        card.origin + point(px(1.), px(1.)),
        card.bottom_right() - point(px(1.), px(1.)),
    );
    if !within(border) || !within(view) {
        failures.push(format!(
            "{at}: {selector}'s ring {ring:?} leaves its card's border {border:?} or the \
             page's view {view:?}"
        ));
    }
    failures
}

/// A press on a switch's row, its title or its description, toggles the
/// switch, and hovering the row lifts an unchecked track by the palette's
/// hover step, as the label Follow system appearance drew beside its
/// track did: with that label gone, only the 36 by 20 px track answered.
#[gpui::test]
fn a_switch_row_answers_the_pointer_as_its_switch(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    cx.update(|window, cx| ThemeChoice::Midnight.apply(Some(window), cx));
    let reopen = |cx: &mut VisualTestContext| cx.read(|cx| app.read(cx).settings.reopen_last);
    let away = |cx: &mut VisualTestContext| {
        cx.simulate_mouse_move(point(px(4.), px(4.)), None, Modifiers::default());
        settle(cx);
    };
    settle(cx);
    let row = shown(cx, "settings-reopen-last-row").expect("the row");
    let before = reopen(cx);
    for part in [
        format!("setting-title: {REOPEN_LAST}"),
        format!("setting-detail: {REOPEN_LAST}"),
    ] {
        let at = drawn(cx, part.clone()).expect("the title");
        let was = reopen(cx);
        cx.simulate_click(at.center(), Modifiers::default());
        settle(cx);
        assert_eq!(reopen(cx), !was, "a press on {part} toggles the switch");
        away(cx);
    }
    assert_eq!(reopen(cx), before);
    // Unchecked, hovered on its title: the track lifts.
    if reopen(cx) {
        let title = drawn(cx, format!("setting-title: {REOPEN_LAST}")).expect("the title");
        cx.simulate_click(title.center(), Modifiers::default());
        away(cx);
    }
    assert!(!reopen(cx), "Restore repository tabs is off");
    // The fill of the 36 by 20 px quad painted at `track`.
    let fill = |cx: &mut VisualTestContext, track: Bounds<Pixels>| {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .find(|quad| {
                    let b = quad.bounds;
                    (b.left().as_f32() / scale - f32::from(track.left())).abs() < 0.01
                        && (b.top().as_f32() / scale - f32::from(track.top())).abs() < 0.01
                        && (b.size.width.as_f32() / scale - 36.).abs() < 0.01
                })
                .map(|quad| quad.background)
        })
    };
    let track = track_in(cx, row).expect("the track");
    let resting = fill(cx, track);
    let title = drawn(cx, format!("setting-title: {REOPEN_LAST}")).expect("the title");
    cx.simulate_mouse_move(title.center(), None, Modifiers::default());
    settle(cx);
    let hovered = fill(cx, track);
    let lift = Background::from(Hsla::from(rgb(cx.read(palette).switch_hover())));
    assert_eq!(hovered, Some(lift), "the hovered title lifts the track");
    assert_ne!(resting, hovered);
    away(cx);
}

/// Linux: while the Omarchy theme locks Follow system, a press on its
/// row's title turns nothing on, as a press on its track does not.
#[cfg(target_os = "linux")]
#[gpui::test]
fn a_locked_follow_system_row_toggles_nothing(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("current");
    std::fs::create_dir_all(root.join("theme")).unwrap();
    let colors = include_bytes!("../tests/fixtures/omarchy/tokyo-night.toml");
    std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
    std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
    cx.update(|cx| drop(appearance::omarchy::install(Some(root.clone()), cx)));
    let (app, cx) = open_app(cx);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings.follow_system = false;
            app.settings.theme = ThemeSelection::Omarchy;
            app.apply_appearance(window, cx);
        })
    });
    settle(cx);
    assert!(page_shows(cx, "follow-system-switch"));
    let title = drawn(cx, format!("setting-title: {FOLLOW_SYSTEM}")).expect("the title");
    cx.simulate_click(title.center(), Modifiers::default());
    settle(cx);
    assert!(
        !cx.read(|cx| app.read(cx).settings.follow_system),
        "the locked row turned Follow system on"
    );
}

/// Tab walks Settings with a repository open in a 1,480 × 800 window at
/// 13 pt, and every text field it stops on is named by the words drawn
/// beside it, read from the node the drawn field writes. The Git
/// identity's Name and Email fields had no name of their own, so their
/// nodes carried only their placeholders: the 47th stop, Name, after the
/// 20 theme cards and the page's other controls, read "Your name".
#[gpui::test]
fn every_settings_field_tab_reaches_is_named(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let repo = crate::tags::tests::tagged_repository(fixture.path());
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| theme_editor::init(cx));
    cx.simulate_resize(size(px(1480.), px(800.)));
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(cx);
    let fields = cx.read(|cx| {
        let app = app.read(cx);
        [
            ("External editor", app.settings_editor_for_test()),
            ("Default branch", app.settings_branch_for_test()),
            ("Name", app.identity_name_for_test()),
            ("Email", app.identity_email_for_test()),
        ]
    });
    let handles: Vec<FocusHandle> = cx.read(|cx| {
        fields
            .iter()
            .map(|(_, field)| field.focus_handle(cx))
            .collect()
    });
    // Each field Tab stops on: its stop, its expected name and the name
    // its node gives.
    let mut reached: Vec<(usize, &str, Option<String>)> = Vec::new();
    let mut order: Vec<FocusHandle> = Vec::new();
    for _ in 0..200 {
        cx.simulate_keystrokes("tab");
        settle(cx);
        let focused = cx
            .update(|window, cx| window.focused(cx))
            .expect("Tab keeps focus");
        if order.contains(&focused) {
            break;
        }
        order.push(focused.clone());
        if let Some(at) = handles.iter().position(|handle| *handle == focused) {
            let (expected, field) = &fields[at];
            let name = crate::views::tests::input_node(cx, field)
                .label()
                .map(str::to_owned);
            reached.push((order.len(), *expected, name));
        }
    }
    for (expected, _) in &fields {
        let (stop, _, name) = reached
            .iter()
            .find(|(_, field, _)| field == expected)
            .unwrap_or_else(|| panic!("Tab reaches the {expected} field: {reached:?}"));
        assert_eq!(
            name.as_deref(),
            Some(*expected),
            "stop {stop}: the {expected} field is named by its label"
        );
    }
}

/// The default branch's field is named by its title, "Default branch",
/// which the row's text then leaves to it: it had no name and no
/// placeholder, so assistive technology announced an unnamed field.
#[gpui::test]
fn the_default_branch_field_is_named_by_its_title(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    assert!(page_shows(cx, "save-default-branch"));
    let field = cx.read(|cx| app.read(cx).settings_branch_for_test());
    crate::native_accessibility::written::forget();
    assert!(page_shows(cx, "save-default-branch"));
    let read: Vec<_> = crate::native_accessibility::written::labels()
        .into_iter()
        .filter_map(|(_, label)| label)
        .filter(|label| label.contains("new repository"))
        .collect();
    assert_eq!(
        read,
        ["Used when creating a new repository."],
        "the row's text reads its description alone"
    );
    let label = crate::views::tests::input_node(cx, &field)
        .label()
        .map(str::to_owned);
    assert_eq!(label.as_deref(), Some("Default branch"));
}

/// A held Return in the repository identity's Name or Email asks for one
/// Git configuration write per press, under X11 too: with the
/// repository's configuration locked, the write fails at once and the
/// field keeps focus, then the key's repeats arrive (`REPEATS`), each
/// waited out until the write has answered. The form writes once and
/// says why; after the key-up a new press writes again. Once the lock
/// goes during a hold, its repeats still write nothing, and the next
/// press saves the identity. The fields saved on their own `PressEnter`,
/// which the field sends for every Enter key-down, so each repeat after
/// the quick failure began another write.
#[gpui::test]
async fn a_held_return_in_the_identity_form_writes_once_per_press(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::{REPEATS, enter};
    use crate::tags::tests::draw;
    use std::cell::Cell;
    let fixture = tempfile::tempdir().unwrap();
    let repo = crate::tags::tests::tagged_repository(fixture.path());
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| theme_editor::init(cx));
    // Git's identity is read before the form is edited, as the page
    // shows it, so the edits are kept as edits when it is read again.
    cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
    crate::views::tests::settle(&app, cx).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(cx);
    assert!(
        app.read_with(cx, |app, _| app.profile.is_some()),
        "Git's identity is read"
    );
    // Git refuses to write a configuration another process has locked.
    let lock = repo.path().join(".git").join("config.lock");
    std::fs::write(&lock, "").unwrap();
    let (name, email) = cx.read(|cx| {
        let app = app.read(cx);
        (app.identity_name_for_test(), app.identity_email_for_test())
    });
    cx.update(|window, cx| {
        name.update(cx, |input, cx| {
            input.set_value("Another Person", window, cx)
        });
        email.update(cx, |input, cx| {
            input.set_value("another@example.com", window, cx)
        });
    });
    settle(cx);
    // Identity writes, counted as each begins.
    let writes = Rc::new(Cell::new(0_usize));
    let _counting = cx.update(|_, cx| {
        let writes = writes.clone();
        let mut was = false;
        cx.observe(&app, move |app, cx| {
            let now = app.read(cx).operation_busy == Some("Saving repository identity…");
            writes.set(writes.get() + usize::from(now && !was));
            was = now;
        })
    });
    // The write's answer lands, and the frames after it.
    let lands = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| app.operations.drain());
        cx.run_until_parked();
        draw(cx);
    };
    let focused = |field: &Entity<InputState>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| field.read(cx).focus_handle(cx).is_focused(window))
    };
    for (field, which) in [(&name, "Name"), (&email, "Email")] {
        cx.update(|window, cx| field.read(cx).focus_handle(cx).focus(window, cx));
        draw(cx);
        for press in ["the first press", "a new press after the key-up"] {
            let at = format!("{which}, {press}");
            let after = writes.get() + 1;
            enter(cx, Some(false));
            lands(cx);
            assert_eq!(writes.get(), after, "{at}: one write");
            assert!(
                app.read_with(cx, |app, _| app.operation_busy.is_none()
                    && app.operation_error.is_some()),
                "{at}: it fails and says why"
            );
            assert!(focused(field, cx), "{at}: the field keeps focus");
            for (repeat, events) in REPEATS {
                for down in events {
                    enter(cx, *down);
                }
                lands(cx);
                assert_eq!(writes.get(), after, "{at}, {repeat}: no other write");
            }
            enter(cx, None);
            draw(cx);
        }
    }
    // The lock goes while the key is held: its repeats still write
    // nothing, and the next press saves the identity.
    let after = writes.get() + 1;
    enter(cx, Some(false));
    lands(cx);
    assert_eq!(writes.get(), after, "the press with the lock held writes");
    std::fs::remove_file(&lock).unwrap();
    for (repeat, events) in REPEATS {
        for down in events {
            enter(cx, *down);
        }
        lands(cx);
        assert_eq!(
            writes.get(),
            after,
            "unlocked, {repeat}: the held key writes nothing"
        );
    }
    enter(cx, None);
    draw(cx);
    enter(cx, Some(false));
    lands(cx);
    enter(cx, None);
    draw(cx);
    assert_eq!(writes.get(), after + 1, "a new press writes again");
    let saved = fixture_config(repo.path(), "user.email");
    assert_eq!(saved, "another@example.com", "and saves the identity");
}

/// The repository's own value of `key`, read with Git.
fn fixture_config(repository: &Path, key: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["config", "--local", "--get", key])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Return in the Default branch field saves it once per press, and saves
/// it again after a save that failed. With a directory where the
/// preference store's file goes, a press saves and fails, and the value
/// stays in use and in the field beside the failure and Retry save; the
/// store is mended while the key is still held, and its repeats
/// (`REPEATS`) save nothing; after the key-up a new press saves it. A
/// press after a failed save found nothing new in the settings and saved
/// nothing, so Return in the field was a dead key, and the field saved
/// on its own `PressEnter`, which it sends for every Enter key-down.
#[gpui::test]
fn return_in_the_default_branch_saves_once_per_press_and_after_a_failure(cx: &mut TestAppContext) {
    use crate::branch_actions::tests::{REPEATS, enter};
    use crate::theme_editor::tests::drain_preference_writer;
    let (app, cx) = open_app(cx);
    drain_preference_writer(cx, &app);
    let store = crate::preferences::settings_path().expect("the store's path");
    if store.exists() {
        std::fs::remove_file(&store).unwrap();
    }
    std::fs::create_dir(&store).unwrap();
    let field = cx.read(|cx| app.read(cx).settings_branch_for_test());
    cx.update(|window, cx| {
        field.update(cx, |input, cx| input.set_value("trunk", window, cx));
        field.read(cx).focus_handle(cx).focus(window, cx);
    });
    settle(cx);
    let failed = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| {
            app.operation_error
                .as_deref()
                .is_some_and(|error| error.starts_with("Could not save settings:"))
        })
    };
    enter(cx, Some(false));
    drain_preference_writer(cx, &app);
    settle(cx);
    assert!(failed(cx), "the save fails and says so, beside Retry save");
    assert_eq!(
        app.read_with(cx, |app, _| app.settings.default_branch.clone()),
        "trunk",
        "the value stays in use"
    );
    // Mended while the key is held: its repeats save nothing.
    std::fs::remove_dir(&store).unwrap();
    for (repeat, events) in REPEATS {
        for down in events {
            enter(cx, *down);
        }
        drain_preference_writer(cx, &app);
        settle(cx);
        assert!(!store.exists(), "{repeat}: the held key saves nothing");
    }
    enter(cx, None);
    settle(cx);
    enter(cx, Some(false));
    drain_preference_writer(cx, &app);
    enter(cx, None);
    settle(cx);
    assert_eq!(
        crate::preferences::Preferences::load()
            .settings
            .default_branch,
        "trunk",
        "a new press saves it"
    );
    assert!(!failed(cx), "and the failure goes");
}

/// Return in the Default branch field saves the settings again after a
/// save that failed though another operation's error has replaced the
/// failure in the window's error line since: the failed save is its own
/// record (`DraftState::failed_save`), not that line's text, and the
/// save that then succeeds leaves the other error standing. Read from
/// the line, the press found nothing new and saved nothing.
#[gpui::test]
fn return_in_the_default_branch_saves_after_another_error_replaced_its_failure(
    cx: &mut TestAppContext,
) {
    use crate::branch_actions::tests::enter;
    use crate::theme_editor::tests::drain_preference_writer;
    let (app, cx) = open_app(cx);
    drain_preference_writer(cx, &app);
    let store = crate::preferences::settings_path().expect("the store's path");
    if store.exists() {
        std::fs::remove_file(&store).unwrap();
    }
    std::fs::create_dir(&store).unwrap();
    let field = cx.read(|cx| app.read(cx).settings_branch_for_test());
    cx.update(|window, cx| {
        field.update(cx, |input, cx| input.set_value("trunk", window, cx));
        field.read(cx).focus_handle(cx).focus(window, cx);
    });
    settle(cx);
    let press = |cx: &mut VisualTestContext| {
        enter(cx, Some(false));
        drain_preference_writer(cx, &app);
        enter(cx, None);
        settle(cx);
    };
    let error = |cx: &mut VisualTestContext| {
        app.read_with(cx, |app, _| {
            app.operation_error.as_deref().map(str::to_owned)
        })
    };
    press(cx);
    assert!(
        error(cx).is_some_and(|error| error.starts_with("Could not save settings:")),
        "the save fails and says so"
    );
    let other = "Could not fetch origin: the remote hung up";
    app.update(cx, |app, cx| {
        app.operation_error = Some(other.into());
        cx.notify();
    });
    settle(cx);
    std::fs::remove_dir(&store).unwrap();
    press(cx);
    assert_eq!(
        crate::preferences::Preferences::load()
            .settings
            .default_branch,
        "trunk",
        "a fresh press saves it"
    );
    assert_eq!(
        error(cx).as_deref(),
        Some(other),
        "the other operation's error stands"
    );
}

/// Linux: while the Omarchy theme locks Follow system, the row's title,
/// which names the switch, takes the disabled label's dim with it, as
/// the label the switch drew beside its track did, and loses it once
/// another theme releases the switch.
#[cfg(target_os = "linux")]
#[gpui::test]
fn a_locked_follow_system_dims_its_title(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("current");
    std::fs::create_dir_all(root.join("theme")).unwrap();
    let colors = include_bytes!("../tests/fixtures/omarchy/tokyo-night.toml");
    std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
    std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
    cx.update(|cx| drop(appearance::omarchy::install(Some(root.clone()), cx)));
    let (app, cx) = open_app(cx);
    settle(cx);
    let key = format!("setting-title: {FOLLOW_SYSTEM}");
    // Whether the switch is locked, the title's colour and the dim.
    let choose = |cx: &mut VisualTestContext, theme: ThemeSelection| {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.theme = theme;
                app.apply_appearance(window, cx);
            })
        });
        settle(cx);
        assert!(page_shows(cx, "follow-system-switch"));
        cx.read(|cx| {
            (
                app.read(cx).follows_desktop_theme(cx),
                appearance::drawn_color(&key),
                gpui_kit::component::switch::disabled_label_color(cx),
            )
        })
    };
    let (locked, color, dim) = choose(cx, ThemeSelection::Omarchy);
    assert!(locked, "the Omarchy theme locks Follow system");
    assert_eq!(color, Some(dim), "the locked switch's title is dimmed");
    let (locked, color, dim) = choose(cx, ThemeSelection::BuiltIn(ThemeChoice::Midnight));
    assert!(!locked, "Midnight releases Follow system");
    assert!(
        color.is_some_and(|color| color != dim),
        "the released switch's title is not dimmed: {color:?}"
    );
}

/// Tab, then Shift-Tab, walk every Settings control in a 1,000 × 680
/// window, crossing the page viewport's lower edge and back, and in every
/// frame painted after each key the focused control, grown by the focus
/// ring's 3 px of room, lies inside the page's viewport.
#[gpui::test]
fn tab_keeps_every_settings_control_in_view(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    assert_tab_keeps_controls_in_view(&app, cx, 30);
}

/// As above, with a repository open, whose Git identity form, Name,
/// Email and its buttons, takes the place of Choose a project.
#[gpui::test]
fn tab_keeps_the_identity_form_in_view(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let repo = crate::tags::tests::tagged_repository(fixture.path());
    let (app, cx) = crate::tags::tests::window(cx, &repo);
    cx.update(|_, cx| theme_editor::init(cx));
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    assert_tab_keeps_controls_in_view(&app, cx, 33);
}

/// Tab walks Settings from its first control until focus returns to it,
/// then Shift-Tab walks back, through at least `controls` page controls.
fn assert_tab_keeps_controls_in_view(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    controls: usize,
) {
    use crate::tags::tests::draw;

    cx.simulate_resize(size(px(1000.), px(680.)));
    settle(cx);
    let page = cx.read(|cx| app.read(cx).theme_editor.page().clone());
    let room = cx.read(appearance::button_ring_room);
    // Takes one key and returns the control it focused, if that control
    // is on the page rather than in the window's header.
    let step = |cx: &mut VisualTestContext, key: &str| -> Option<FocusHandle> {
        page.take_painted();
        cx.simulate_keystrokes(key);
        draw(cx);
        let (focused, on_page) = cx.update(|window, cx| {
            let on_page = window
                .context_stack()
                .iter()
                .any(|context| context.contains(theme_editor::SETTINGS_CONTEXT));
            (window.focused(cx).expect("{key} keeps focus"), on_page)
        });
        if !on_page {
            return None;
        }
        let frames = page.take_painted();
        assert!(!frames.is_empty(), "{key} paints the page");
        let slack = px(0.01);
        for (index, frame) in frames.iter().enumerate() {
            let ring = frame
                .control
                .unwrap_or_else(|| panic!("frame {index} after {key} draws the focused control"))
                .dilate(room);
            assert!(
                ring.top() + slack >= frame.viewport.top()
                    && ring.bottom() <= frame.viewport.bottom() + slack,
                "frame {index} after {key} paints the focused control with its ring, \
                 {ring:?}, outside the page, {:?}",
                frame.viewport
            );
        }
        Some(focused)
    };
    // With nothing focused a key reaches no binding; start the walk.
    cx.update(|window, cx| window.focus_next(cx));
    settle(cx);
    let mut order: Vec<FocusHandle> = Vec::new();
    let mut deepest = px(0.);
    for _ in 0..200 {
        let Some(focused) = step(cx, "tab") else {
            continue;
        };
        if order.first() == Some(&focused) {
            break;
        }
        order.push(focused);
        deepest = deepest.min(page.scroll().offset().y);
    }
    assert!(
        order.len() >= controls,
        "Tab reached {} Settings controls",
        order.len()
    );
    let viewport = page.scroll().bounds().size.height;
    assert!(
        deepest < -viewport,
        "Tab scrolled the page past its first screen ({deepest:?}, {viewport:?} tall)"
    );
    // Back from the first control: Shift-Tab leaves the page for the
    // header, then walks the page from its foot to its top.
    let mut back: Vec<FocusHandle> = Vec::new();
    let mut highest = deepest;
    for _ in 0..200 {
        if let Some(focused) = step(cx, "shift-tab") {
            if back.first() == Some(&focused) {
                break;
            }
            back.push(focused);
            highest = highest.max(page.scroll().offset().y);
        }
    }
    back.reverse();
    assert_eq!(back, order, "Shift-Tab walks the same controls back");
    assert!(
        highest > -viewport,
        "Shift-Tab scrolled back to the page's first screen ({highest:?})"
    );
}

/// A Tab-focused, enabled Settings Switch draws the Button focus ring, 2 px
/// of full accent 1 px outside its track, whole inside every clip, at the
/// default and the largest interface size, and gaining focus moves nothing;
/// no unfocused Switch draws one. Hovering an enabled unchecked Switch lifts
/// its track to `Palette::switch_hover`; a checked or disabled one keeps its
/// resting fill.
#[gpui::test]
fn a_focused_switch_draws_the_button_ring_whole_and_moves_nothing(cx: &mut TestAppContext) {
    use gpui_kit::{ScaledPixels, component::ActiveTheme as _};

    /// Switch tracks (the 36 x 20 filled quads) with their fills, the
    /// Button-ring quads with whether their clip holds them whole, and the
    /// bounds of every other quad, in paint order.
    type Frame = (
        Vec<(Bounds<Pixels>, Background)>,
        Vec<(Bounds<Pixels>, bool)>,
        Vec<Bounds<Pixels>>,
    );
    fn frame(cx: &mut VisualTestContext) -> Frame {
        cx.update(|window, cx| {
            let scale = window.scale_factor();
            let logical = |b: Bounds<ScaledPixels>| {
                Bounds::from_corners(
                    point(px(b.left().as_f32() / scale), px(b.top().as_f32() / scale)),
                    point(
                        px(b.right().as_f32() / scale),
                        px(b.bottom().as_f32() / scale),
                    ),
                )
            };
            let ring = appearance::BUTTON_FOCUS_RING;
            let ring_color = cx.theme().ring.alpha(ring.opacity);
            let (mut tracks, mut sides, mut others) = (Vec::new(), Vec::new(), Vec::new());
            for quad in window.painted_quads() {
                let bounds = logical(quad.bounds);
                if quad.background.is_transparent()
                    && quad.border_color == ring_color
                    && quad.border_widths.top.as_f32() == ring.width.as_f32() * scale
                {
                    sides.push((quad.bounds, quad.content_mask.bounds));
                } else if !quad.background.is_transparent() && bounds.size == size(px(36.), px(20.))
                {
                    tracks.push((bounds, quad.background));
                } else {
                    others.push(bounds);
                }
            }
            // The kit paints a ring as one quad per side, each masked to
            // its side: the ring is whole when those masks cover it.
            let mut rings: Vec<(Bounds<Pixels>, bool)> = Vec::new();
            for (ring, _) in &sides {
                if rings.iter().any(|(seen, _)| *seen == logical(*ring)) {
                    continue;
                }
                let covered = sides
                    .iter()
                    .filter(|(other, _)| other == ring)
                    .map(|(_, mask)| *mask)
                    .reduce(|a, b| a.union(&b))
                    .unwrap();
                let whole = covered.left() <= ring.left()
                    && covered.top() <= ring.top()
                    && covered.right() >= ring.right()
                    && covered.bottom() >= ring.bottom();
                rings.push((logical(*ring), whole));
            }
            (tracks, rings, others)
        })
    }
    let around = |track: Bounds<Pixels>| {
        let room = appearance::BUTTON_FOCUS_RING.gap + appearance::BUTTON_FOCUS_RING.width;
        track.dilate(room)
    };

    let (app, cx) = open_app(cx);
    let midnight = ThemeSelection::BuiltIn(ThemeChoice::Midnight);
    let select = |cx: &mut VisualTestContext, follow_system: bool| {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.theme = midnight;
                app.settings.follow_system = follow_system;
                app.apply_appearance(window, cx);
            })
        });
        settle(cx);
    };
    select(cx, false);
    for (interface, height) in [(13, 3000.), (18, 5000.)] {
        cx.simulate_resize(size(px(1440.), px(height)));
        cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
        cx.update(|window, cx| window.blur(cx));
        settle(cx);
        let (tracks, rings, others) = frame(cx);
        let switches: Vec<_> = tracks.iter().map(|(track, _)| *track).collect();
        assert!(switches.len() >= 3, "{interface} pt: {switches:?}");
        assert!(
            !rings
                .iter()
                .any(|(ring, _)| switches.iter().any(|t| around(*t) == *ring)),
            "{interface} pt: an unfocused switch draws a ring"
        );
        let mut seen = Vec::new();
        // With nothing focused a key reaches no binding; start the walk.
        cx.update(|window, cx| window.focus_next(cx));
        for _ in 0..300 {
            if seen.len() == switches.len() {
                break;
            }
            cx.simulate_keystrokes("tab");
            settle(cx);
            let (now_tracks, now_rings, now_others) = frame(cx);
            let ringed: Vec<_> = now_rings
                .iter()
                .filter(|(ring, _)| switches.iter().any(|t| around(*t) == *ring))
                .collect();
            let [(ring, whole)] = ringed[..] else {
                assert!(ringed.is_empty(), "{interface} pt: {ringed:?}");
                continue;
            };
            assert!(whole, "{interface} pt: a clip cuts the ring {ring:?}");
            assert_eq!(
                now_tracks.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
                switches,
                "{interface} pt: a switch moved"
            );
            let missing = |from: &[Bounds<Pixels>], to: &[Bounds<Pixels>]| {
                let mut left = to.to_vec();
                from.iter()
                    .filter(|b| match left.iter().position(|o| o == *b) {
                        Some(at) => {
                            left.remove(at);
                            false
                        }
                        None => true,
                    })
                    .copied()
                    .collect::<Vec<_>>()
            };
            let (gone, came) = (missing(&others, &now_others), missing(&now_others, &others));
            assert!(
                gone.is_empty() && came.is_empty(),
                "{interface} pt: focus moved quads: {gone:?} became {came:?}"
            );
            if !seen.contains(ring) {
                seen.push(*ring);
            }
        }
        assert_eq!(
            seen.len(),
            switches.len(),
            "{interface} pt: Tab rings every switch"
        );
    }
    cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    cx.simulate_resize(size(px(1440.), px(3000.)));
    cx.update(|window, cx| window.blur(cx));

    // The Follow system switch, hovered: unchecked, then checked.
    let hovered_fill = |cx: &mut VisualTestContext| {
        settle(cx);
        let area = shown(cx, "follow-system-switch").unwrap();
        let (tracks, _, _) = frame(cx);
        let (track, _) = *tracks
            .iter()
            .find(|(track, _)| area.contains(&track.center()))
            .unwrap();
        cx.simulate_mouse_move(track.center(), None, Modifiers::default());
        settle(cx);
        let (tracks, _, _) = frame(cx);
        let fill = tracks.iter().find(|(t, _)| *t == track).unwrap().1;
        cx.simulate_mouse_move(point(px(4.), px(4.)), None, Modifiers::default());
        fill
    };
    let fill = |color: u32| Background::from(Hsla::from(rgb(color)));
    select(cx, false);
    let unchecked = hovered_fill(cx);
    assert_eq!(unchecked, fill(cx.read(palette).switch_hover()));
    assert_ne!(unchecked, fill(cx.read(palette).border));
    select(cx, true);
    let checked = hovered_fill(cx);
    assert_eq!(
        checked,
        fill(cx.read(palette).accent),
        "a checked track keeps its accent"
    );

    #[cfg(target_os = "linux")]
    {
        // The Omarchy theme disables Follow system: its track keeps its
        // faded resting fill under the pointer.
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("current");
        std::fs::create_dir_all(root.join("theme")).unwrap();
        let colors = include_bytes!("../tests/fixtures/omarchy/tokyo-night.toml");
        std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
        std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
        cx.update(|_, cx| drop(appearance::omarchy::install(Some(root.clone()), cx)));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.theme = ThemeSelection::Omarchy;
                app.settings.follow_system = false;
                app.apply_appearance(window, cx);
            })
        });
        let disabled = hovered_fill(cx);
        assert_eq!(disabled, fill(cx.read(palette).border).opacity(0.5));
    }
}

/// The Omarchy card's caption fits its card at the default interface size
/// in a 1,400 px window, for the longest bundled theme name (Catppuccin
/// Latte): an unavailable card's name line holds "Omarchy", "Unavailable"
/// and the check badge, and every description the card can show, from
/// "Reading your theme…" to "Keeping the last colors", is shaped narrower
/// than the box that lays it out, so none is cut. The test text system
/// gives every character 0.6 em, wider than the interface font's letters
/// on average.
#[cfg(target_os = "linux")]
fn omarchy_captions_fit(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) {
    let omarchy = ThemeSelection::Omarchy;
    cx.simulate_resize(size(px(1400.), px(2400.)));
    settle(cx);
    let body = miniature(cx, app, omarchy);
    let width = |cx: &mut VisualTestContext, text: &str, size: f32| {
        cx.update(|window, _| {
            let run = TextRun {
                len: text.len(),
                font: window.text_style().font(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let size = appearance::ui_text(size);
            window
                .text_system()
                .shape_line(text.to_owned().into(), size, &[run], None)
                .width()
        })
    };
    let description = drawn(cx, format!("theme-description-{body}")).expect("a description");
    let descriptions = appearance::omarchy::every_description("Catppuccin Latte");
    assert!(descriptions.contains(&"Reading your theme…".to_owned()));
    for shown in descriptions {
        assert!(
            width(cx, &shown, 10.) <= description.size.width,
            "{shown:?} fits {description:?}"
        );
    }
    let name = drawn(cx, format!("theme-name-{body}")).expect("a name");
    let tag = drawn(cx, format!("theme-tag-{body}")).expect("the Unavailable tag");
    assert!(width(cx, "Omarchy", 12.) <= name.size.width, "{name:?}");
    assert!(width(cx, "Unavailable", 10.) <= tag.size.width, "{tag:?}");
    assert!(name.right() <= tag.left());
    cx.simulate_resize(size(px(1440.), px(2400.)));
    settle(cx);
}

/// Linux: Follow system turns disabled while it holds focus when the
/// Omarchy theme is chosen, as a click on its card chooses it, without
/// moving keyboard focus. Tab and Shift+Tab, as keystrokes through the
/// window, still leave the Switch for its neighbours, and skip it while it
/// is disabled.
#[cfg(target_os = "linux")]
#[gpui::test]
fn tab_and_shift_tab_leave_follow_system_once_omarchy_disables_it(cx: &mut TestAppContext) {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("current");
    std::fs::create_dir_all(root.join("theme")).unwrap();
    let colors = include_bytes!("../tests/fixtures/omarchy/tokyo-night.toml");
    std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
    std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
    cx.update(|cx| drop(appearance::omarchy::install(Some(root.clone()), cx)));
    let (app, cx) = open_app(cx);
    settle(cx);
    let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
    let press = |cx: &mut VisualTestContext, keys: &str| {
        cx.simulate_keystrokes(keys);
        settle(cx);
        focused(cx).expect("a focused control")
    };
    let locked = |cx: &mut VisualTestContext| {
        page_shows(cx, "follow-system-switch");
        cx.read(|cx| app.read(cx).follow_system_row.borrow().clone().unwrap().1)
    };

    // A click focuses the enabled Switch; it reads between two tab stops.
    let switch = shown(cx, "follow-system-switch").expect("the Follow system switch");
    cx.simulate_click(switch.center(), Modifiers::default());
    settle(cx);
    let switch = focused(cx).expect("the clicked Switch holds focus");
    assert!(!locked(cx));
    let previous = press(cx, "shift-tab");
    assert_ne!(previous, switch);
    assert_eq!(press(cx, "tab"), switch);
    let next = press(cx, "tab");
    assert!(next != switch && next != previous);
    assert_eq!(press(cx, "shift-tab"), switch);

    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.choose_theme(ThemeSelection::Omarchy, window, cx)
        })
    });
    settle(cx);
    assert!(locked(cx), "Omarchy disables Follow system");
    assert_eq!(
        focused(cx),
        Some(switch.clone()),
        "the disabled Switch keeps focus until the keyboard moves it"
    );
    assert_eq!(press(cx, "tab"), next, "Tab leaves the disabled Switch");
    assert_eq!(
        press(cx, "shift-tab"),
        previous,
        "Shift+Tab passes over the disabled Switch"
    );

    cx.update(|window, cx| window.focus(&switch, cx));
    settle(cx);
    assert!(locked(cx));
    assert_eq!(
        press(cx, "shift-tab"),
        previous,
        "Shift+Tab leaves the disabled Switch"
    );
    assert_eq!(
        press(cx, "tab"),
        next,
        "Tab passes over the disabled Switch"
    );
}

/// Linux: the Omarchy card's description takes a second line when one
/// cannot hold it and ends in an ellipsis only when two cannot, at
/// 1,400 px and at the window's 1,000 px minimum, for the longest name
/// the card shows (64 bytes), while the theme is read ("Follows …") and
/// while it is not ("Keeping …"). The card keeps its height and place
/// and nothing below it moves: a two-line caption grows into the
/// miniature, which then draws three whole changed-file rows instead of
/// four, and a one-line caption is laid out as a built-in card's, four
/// rows included. At the smallest and largest interface text sizes the
/// one-line caption still matches a built-in card's, the fourth row the
/// caption covers at 11 pt included, and a two-line caption's rows are
/// whole. The test text system's 0.6 em characters are wider
/// than the interface font's, so a name cut here may fit on screen.
#[cfg(target_os = "linux")]
#[gpui::test]
fn a_long_omarchy_caption_takes_a_second_line(cx: &mut TestAppContext) {
    #[derive(Debug)]
    struct Layout {
        card: Bounds<Pixels>,
        light: Pixels,
        caption: Bounds<Pixels>,
        description: Bounds<Pixels>,
        place: Bounds<Pixels>,
        rows: Vec<Bounds<Pixels>>,
    }
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("current");
    std::fs::create_dir_all(root.join("theme")).unwrap();
    let colors = include_bytes!("../tests/fixtures/omarchy/tokyo-night.toml");
    std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
    std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
    cx.update(|cx| drop(appearance::omarchy::install(Some(root.clone()), cx)));
    let (app, cx) = open_app(cx);
    let omarchy = ThemeSelection::Omarchy;
    let body = miniature(cx, &app, omarchy);
    // An unselected built-in card, laid out as every card was before.
    let nord = ThemeSelection::BuiltIn(ThemeChoice::Nord);
    let nord_body = miniature(cx, &app, nord);
    // 64 bytes once title-cased.
    let long = "aurora-borealis-over-the-northern-fjords-at-midnight-in-deep-win";
    let long_title = "Aurora Borealis Over The Northern Fjords At Midnight In Deep Win";
    assert_eq!(long_title.len(), 64);
    // The card's description once a read of `name` has landed and, when
    // `broken`, once a further read finds `colors.toml` malformed, which
    // keeps the card offered and names the palette it keeps. Settings
    // watches `current/`, so a write can also start the watch's quiet
    // period, which a refresh joins: each try advances the clock past
    // it, and waits in real time for the watcher's thread.
    // Returns the description and, laid out by then, the lines it drew.
    let show = |cx: &mut VisualTestContext, name: &str, title: &str, broken: bool| {
        let body = |cx: &mut VisualTestContext| {
            cx.read(|cx| app.read(cx).theme_card_body(omarchy).clone())
        };
        let described = |cx: &mut VisualTestContext| {
            let body = body(cx);
            cx.read(|cx| {
                body.read(cx)
                    .key
                    .as_ref()
                    .map(|key| key.description.to_string())
            })
        };
        let landed = |cx: &mut VisualTestContext, description: String| {
            for _ in 0..500 {
                cx.update(|_, cx| appearance::omarchy::refresh(cx));
                settle(cx);
                cx.executor()
                    .advance_clock(appearance::omarchy::QUIET_PERIOD);
                settle(cx);
                if described(cx).as_ref() == Some(&description) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("the card never showed {description:?}");
        };
        std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
        std::fs::write(root.join("theme.name"), name).unwrap();
        landed(cx, format!("Follows {title}"));
        if broken {
            std::fs::write(
                root.join("theme/colors.toml"),
                "[colors]\nbackground = \"#000000\"\n",
            )
            .unwrap();
            landed(cx, format!("Keeping {title}"));
        }
        let body = body(cx);
        cx.read(|cx| {
            let body = body.read(cx);
            let description = body.key.as_ref().unwrap().description.to_string();
            (description, body.drawn_description())
        })
    };
    let layout = |cx: &mut VisualTestContext,
                  card: String,
                  miniature: EntityId,
                  selection: ThemeSelection| {
        let bounds = |cx: &mut VisualTestContext, selector: String| {
            drawn(cx, selector.clone()).unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        Layout {
            card: bounds(cx, card),
            light: bounds(cx, "settings-theme-group-light".into()).top(),
            caption: bounds(cx, format!("theme-caption-{miniature}")),
            description: bounds(cx, format!("theme-description-{miniature}")),
            place: bounds(cx, format!("theme-miniature-{miniature}")),
            rows: cx.read(|cx| {
                app.read(cx)
                    .theme_preview_body(selection)
                    .read(cx)
                    .painted_rows()
            }),
        }
    };
    let omarchy_layout =
        |cx: &mut VisualTestContext| layout(cx, "settings-theme-omarchy".into(), body, omarchy);
    let nord_layout = |cx: &mut VisualTestContext| {
        let card = format!("settings-theme-{}", ThemeChoice::Nord as usize);
        layout(cx, card, nord_body, nord)
    };
    // The lines `text` wraps to in `width` with no limit, in the font the
    // Omarchy card's description is drawn in: its slot's text style,
    // which [`LayerCard`] pins, at the description's normal weight.
    let needed = |cx: &mut VisualTestContext, text: &str, width: Pixels| {
        cx.update(|window, cx| {
            let slots = app.read(cx).picker_cards.0.borrow();
            let slot = slots
                .iter()
                .find(|slot| slot.miniature.entity_id() == body)
                .expect("the Omarchy card has a slot");
            let mut style = slot.text.clone();
            style.font_weight = FontWeight::NORMAL;
            let run = TextRun {
                len: text.len(),
                font: style.font(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_text(
                    text.to_owned().into(),
                    appearance::ui_text(10.),
                    &[run],
                    Some(width),
                    None,
                )
                .unwrap()
                .iter()
                .map(|line| line.wrap_boundaries().len() + 1)
                .sum::<usize>()
        })
    };
    // Every drawn row lies whole inside the miniature's box, above the
    // caption, and inside the card.
    let whole = |layout: &Layout| {
        for row in &layout.rows {
            assert!(
                layout.place.top() <= row.top()
                    && row.bottom() <= layout.place.bottom()
                    && row.bottom() <= layout.caption.top()
                    && layout.card.left() <= row.left()
                    && row.right() <= layout.card.right(),
                "{row:?} is whole in {layout:?}"
            );
        }
    };
    // Offsets from the card's top and heights, which a width leaves
    // alone, in hundredths of a pixel: cards at different heights round
    // the same offset differently in the last bits.
    let vertical = |layout: &Layout| {
        let top = layout.card.top();
        let at = |part: &Bounds<Pixels>| {
            [part.top() - top, part.size.height]
                .map(|length| (f32::from(length) * 100.).round() as i32)
        };
        (
            at(&layout.card),
            [layout.caption, layout.description, layout.place].map(|part| at(&part)),
            layout.rows.iter().map(at).collect::<Vec<_>>(),
        )
    };
    let line = f32::from(appearance::ui_text(10.)) * 1.3;
    for broken in [false, true] {
        for width in [1400., 1000.] {
            cx.simulate_resize(size(px(width), px(2400.)));
            settle(cx);
            let short = show(cx, "tokyo-night", "Tokyo Night", broken);
            let one = omarchy_layout(cx);
            let wrapped = show(cx, long, long_title, broken);
            let two = omarchy_layout(cx);
            let at = format!("{} at {width} px", if broken { "kept" } else { "read" });
            let verb = if broken { "Keeping" } else { "Follows" };
            assert_eq!(short.0, format!("{verb} Tokyo Night"), "{at}");
            assert_eq!(wrapped.0, format!("{verb} {long_title}"), "{at}");
            for ((text, lines_drawn), layout) in [(&short, &one), (&wrapped, &two)] {
                let lines = lines_drawn.lines().count();
                let height = f32::from(layout.description.size.height);
                let needed = needed(cx, text, layout.description.size.width);
                assert!((1..=2).contains(&lines), "{at}: {lines_drawn:?}");
                assert!(
                    (height - line * lines as f32).abs() < 0.01,
                    "{at}: {layout:?}"
                );
                if needed <= 2 {
                    assert_eq!(lines, needed, "{at}: {lines_drawn:?}");
                    assert_eq!(lines_drawn.replace('\n', ""), *text, "{at}: nothing is cut");
                } else {
                    assert_eq!(lines, 2, "{at}: {lines_drawn:?}");
                    let kept = lines_drawn.replace('\n', "");
                    let kept = kept.strip_suffix('…').expect("an ellipsis ends the cut");
                    assert!(text.starts_with(kept), "{at}: {lines_drawn:?}");
                }
            }
            // The short name takes one line; the long one two, cut at
            // 1,400 px and whole at the minimum width.
            assert_eq!(short.1.lines().count(), 1, "{at}");
            assert_eq!(wrapped.1.lines().count(), 2, "{at}");
            assert_eq!(
                wrapped.1.ends_with('…'),
                width == 1400.,
                "{at}: {:?}",
                wrapped.1
            );

            // The card neither grows nor moves, and nor does the group
            // below it; the miniature gives the second line its fourth
            // row, and with one line it is laid out as a built-in card's.
            assert_eq!(one.card, two.card, "{at}");
            assert_eq!(one.card.size.height, appearance::ui_size(132.), "{at}");
            assert_eq!(one.light, two.light, "{at}");
            assert_eq!((one.rows.len(), two.rows.len()), (4, 3), "{at}");
            whole(&two);
            assert_eq!(vertical(&one), vertical(&nord_layout(cx)), "{at}");
        }
    }

    let code = cx.read(|cx| app.read(cx).settings.code_text_size);
    cx.simulate_resize(size(px(1400.), px(2400.)));
    for interface in [11, 18] {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        settle(cx);
        let at = format!("at {interface} pt");
        // One line, then two, then one again.
        show(cx, "tokyo-night", "Tokyo Night", false);
        let one = omarchy_layout(cx);
        assert_eq!(vertical(&one), vertical(&nord_layout(cx)), "{at}");
        show(cx, long, long_title, false);
        let two = omarchy_layout(cx);
        assert_eq!(one.card, two.card, "{at}");
        assert!(one.caption.size.height < two.caption.size.height, "{at}");
        assert!(!two.rows.is_empty(), "{at}: {two:?}");
        whole(&two);
        show(cx, "tokyo-night", "Tokyo Night", false);
        let again = omarchy_layout(cx);
        assert_eq!(vertical(&again), vertical(&one), "{at}");
    }
}

/// Without the Omarchy reader (no absolute `$HOME` on Linux, or another
/// platform), a stored Omarchy selection is the default theme: Follow
/// system stays free and, turned on, follows the system; the default's
/// card is marked while it is off; no Desktop group shows; and the stored
/// selection is kept for a desktop that has the theme.
#[gpui::test]
fn an_omarchy_selection_without_its_reader_is_the_default_theme(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.settings.theme = ThemeSelection::Omarchy;
            app.settings.follow_system = false;
            app.apply_appearance(window, cx);
        })
    });
    settle(cx);
    let midnight = ThemeSelection::BuiltIn(ThemeChoice::Midnight);
    let resolved = |cx: &mut VisualTestContext| {
        cx.read(|cx| {
            let app = app.read(cx);
            let default = AppSettings {
                theme: ThemeSelection::default(),
                ..app.settings.clone()
            };
            (
                app.effective_theme(cx),
                default.resolved_theme(
                    matches!(
                        cx.window_appearance(),
                        gpui_kit::WindowAppearance::Light
                            | gpui_kit::WindowAppearance::VibrantLight
                    ),
                    &[],
                ),
                app.selected_theme_card(cx),
                app.follows_desktop_theme(cx),
            )
        })
    };
    let (effective, default, selected, locked) = resolved(cx);
    assert_eq!(
        (effective, selected, locked),
        (default, Some(midnight), false)
    );
    let check = miniature(cx, &app, midnight);
    assert!(drawn(cx, format!("theme-check-{check}")).is_some());
    assert!(!page_shows(cx, "settings-theme-group-desktop"));
    page_shows(cx, "follow-system-switch");
    let unlocked = "Braden in Light Mode; your selected dark palette in Dark Mode.";
    let row = cx.read(|cx| app.read(cx).follow_system_row.borrow().clone().unwrap());
    assert_eq!(
        (row.0, row.1, row.2.to_string()),
        (false, false, unlocked.into())
    );

    let switch = shown(cx, "follow-system-switch").expect("the Follow system switch");
    cx.simulate_click(switch.center(), Modifiers::default());
    settle(cx);
    let (effective, default, selected, locked) = resolved(cx);
    assert_eq!((effective, selected, locked), (default, None, false));
    assert!(cx.read(|cx| app.read(cx).settings.follow_system));
    assert_eq!(
        cx.read(|cx| app.read(cx).settings.theme),
        ThemeSelection::Omarchy
    );
}

/// At the 32-theme bound New theme… and Import… are disabled, so their
/// tooltips are the only place that gives the reason and the remedy.
#[gpui::test]
fn bound_tooltips_say_what_to_delete(cx: &mut TestAppContext) {
    let (app, cx) = open_app(cx);
    // A frame that builds the page, which a seeded store alone does not.
    let tooltips = |cx: &mut VisualTestContext| {
        assert!(page_shows(cx, "custom-themes-import"));
        cx.read(|cx| app.read(cx).theme_action_tooltips.borrow().clone())
    };
    let expected = |new: &str, import: &str| {
        [
            ("custom-themes-new".to_owned(), new.to_owned()),
            ("custom-themes-import".to_owned(), import.to_owned()),
        ]
    };
    seed_themes(cx, &app, preferences::MAX_CUSTOM_THEMES as u32 - 1);
    assert_eq!(
        tooltips(cx),
        expected(
            "Create a theme from a built-in base",
            "Add a theme from an exported JSON file"
        )
    );

    seed_themes(cx, &app, preferences::MAX_CUSTOM_THEMES as u32);
    assert_eq!(
        tooltips(cx),
        expected(
            "Up to 32 custom themes can be saved. Delete one before adding another.",
            "Up to 32 custom themes can be saved. Delete one before importing another."
        )
    );
    // Both are disabled: a click reaches neither the editor, which would
    // refuse with the bound, nor the import dialog.
    for selector in ["custom-themes-new", "custom-themes-import"] {
        let button = shown(cx, selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(button.center(), Modifiers::default());
        settle(cx);
        cx.read(|cx| {
            let editor = &app.read(cx).theme_editor;
            assert_eq!(editor.error(), None, "{selector}");
            assert_eq!(editor.preview(), None, "{selector}");
            assert!(!editor.transfer_pending(), "{selector}");
        });
    }
}
