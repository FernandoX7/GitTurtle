//! The focus-visibility sweep: every page and dialog, at the default and the
//! largest interface size, walked with Tab until focus comes round again.
//! At every stop something drawn holds focus, never nothing or the window's
//! root, and it paints its ring whole: all four sides drawn and none cut by
//! a clip around it, the window's edge included.
//!
//! A stop's ring is what focus adds to the frame: the outlines in the accent,
//! at any opacity, that the frame draws with the stop focused, not with the
//! window blurred, and again with focus back on the stop, its layout
//! otherwise the same; under a dialog, only those it paints above its
//! backdrop. Nothing that lands on the page between those frames, such as a
//! refresh moving History's selected row, counts. A Button's 2 px ring,
//! an Input's accent edge and the kit's ring outside it, and a list's or a
//! pane's 1 px edge all count, so a stop shows its focus however it draws it.
//! The modules whose dialogs need their own fixtures walk them with
//! [`walk`]: `discard::tests`, `worktrees::tests` and `github_view::tests`.
use super::{PaintedOutline, painted_outlines_where};
use crate::appearance::{ThemeChoice, custom::ThemeSelection};
use crate::*;
use ::core::prelude::v1::test;

/// The interface sizes the sweep walks, in points: the default and the
/// largest.
pub(crate) const SIZES: [u8; 2] = [13, 18];

/// More stops than any surface has: a walk that has not come round by then
/// never will.
const MAX_STOPS: usize = 300;

/// Draws the window `frames` times, running what each frame started, so a
/// scroll that a focus change requests lands before the frame is read.
fn draw(cx: &mut VisualTestContext, frames: usize) {
    for _ in 0..frames {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.executor().run_until_parked();
    }
}

/// The last frame's outlines in the accent, at any opacity.
fn accent_outlines(cx: &mut VisualTestContext) -> Vec<PaintedOutline> {
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    painted_outlines_where(cx, move |color| {
        color.a > 0.
            && Hsla {
                a: accent.a,
                ..color
            } == accent
    })
}

/// The scene order of the open dialog's backdrop, the fill in the kit's
/// overlay colour over the whole window, in the last frame: whatever the
/// page beneath the dialog paints lies below it, and what the dialog paints
/// above. None with no dialog open, or one that dims nothing.
fn backdrop_order(cx: &mut VisualTestContext) -> Option<u32> {
    use gpui_kit::component::Theme;
    let overlay = cx.read(|cx| Background::from(Theme::global(cx).colors.overlay));
    cx.update(|window, cx| {
        if !window.has_active_dialog(cx) || overlay.is_transparent() {
            return None;
        }
        let scale = window.scale_factor();
        let viewport = window.viewport_size();
        let covers = |bounds: Bounds<gpui_kit::ScaledPixels>| {
            bounds.size.width.as_f32() >= viewport.width.as_f32() * scale - 1.
                && bounds.size.height.as_f32() >= viewport.height.as_f32() * scale - 1.
        };
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background == overlay && covers(quad.bounds))
            .map(|quad| quad.order)
            .max()
    })
}

/// What holding focus on `focus` adds to the frame: the accent outlines that
/// show with it focused, that a blurred window does not draw, and that show
/// again once focus is back on `focus`, where it stays. Only focus may make
/// the difference: replies keep landing on the page while the frames are
/// drawn, and an outline a reply moves, such as History's selected row's
/// leading border, which a quiet refresh listing new commits above the row
/// moves down, differed between the focused and the blurred frame. Under a
/// dialog, only what the dialog paints above its backdrop counts, since
/// focus in a dialog draws nothing on the page beneath it.
///
/// `between` runs before each frame after the first, with false before the
/// blurred frame and true before the refocused one, for the sweep's own
/// tests to change the page there as a landing reply does.
fn ring(
    cx: &mut VisualTestContext,
    focus: &FocusHandle,
    between: &mut dyn FnMut(&mut VisualTestContext, bool),
) -> Vec<PaintedOutline> {
    let focused = accent_outlines(cx);
    let beneath = backdrop_order(cx);
    between(cx, false);
    cx.update(|window, cx| window.blur(cx));
    draw(cx, 1);
    let blurred = accent_outlines(cx);
    between(cx, true);
    cx.update(|window, cx| window.focus(focus, cx));
    draw(cx, 1);
    let refocused = accent_outlines(cx);
    let same = |outline: &PaintedOutline, other: &PaintedOutline| {
        other.bounds == outline.bounds && other.color == outline.color
    };
    focused
        .into_iter()
        .filter(|outline| {
            outline.drawn
                && beneath.is_none_or(|backdrop| outline.order > backdrop)
                && !blurred.iter().any(|other| same(outline, other))
                && refocused.iter().any(|other| same(outline, other))
        })
        .collect()
}

/// Walks the shown surface with Tab from wherever it put focus until focus
/// comes round again, checking every stop, and adds each failing stop to
/// `failures`, named by `surface`, the interface size and its place in the
/// order. Returns the bounds of every ring the walk saw, so a caller can
/// check that Tab reached a particular pane.
pub(crate) fn walk(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    interface: u8,
    failures: &mut Vec<String>,
) -> Vec<Bounds<Pixels>> {
    walk_with(app, cx, surface, interface, failures, &mut |_, _| {})
}

/// [`walk`], running `between` between the frames each stop's ring is read
/// from ([`ring`]).
fn walk_with(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    interface: u8,
    failures: &mut Vec<String>,
    between: &mut dyn FnMut(&mut VisualTestContext, bool),
) -> Vec<Bounds<Pixels>> {
    let root = app.read_with(cx, |app, _| app.app_focus.clone());
    let mut order: Vec<FocusHandle> = Vec::new();
    let mut rings = Vec::new();
    for stop in 0..MAX_STOPS {
        cx.simulate_keystrokes("tab");
        draw(cx, 2);
        // Moving the pointer without a press keeps the ring: GPUI's
        // `focus_visible` dropped it here.
        cx.simulate_mouse_move(point(px(2.), px(2.)), None, Modifiers::none());
        draw(cx, 1);
        // The key contexts around focus name the stop well enough to find.
        let contexts = cx.update(|window, _| {
            let stack = window.context_stack();
            let names: Vec<_> = stack
                .iter()
                .rev()
                .take(3)
                .map(|context| format!("{context:?}"))
                .collect();
            names.join(" < ")
        });
        let at = format!("{surface} at {interface} pt, stop {stop} ({contexts})");
        let Some(focused) = cx.update(|window, cx| window.focused(cx)) else {
            failures.push(format!("{at}: Tab left nothing focused"));
            return rings;
        };
        if let Some(seen) = order.iter().position(|other| *other == focused) {
            if seen > 0 && seen + 1 == order.len() {
                failures.push(format!("{at}: Tab keeps focus where it was"));
            }
            return rings;
        }
        if focused == root {
            failures.push(format!(
                "{at}: the window's root holds focus and shows none"
            ));
        } else if contexts.is_empty() {
            failures.push(format!(
                "{at}: focus is on an element the frame does not draw"
            ));
        } else {
            let ring = ring(cx, &focused, between);
            rings.extend(ring.iter().map(|outline| outline.bounds));
            if ring.is_empty() {
                failures.push(format!("{at}: the focused element paints no ring"));
            }
            for outline in ring.iter().filter(|outline| !outline.whole) {
                let cut: Vec<_> = ["top", "right", "bottom", "left"]
                    .into_iter()
                    .zip(outline.sides)
                    .filter_map(|(side, whole)| (!whole).then_some(side))
                    .collect();
                failures.push(format!(
                    "{at}: a clip cuts the {} of its ring at {:?}, {:?} wide",
                    cut.join(", "),
                    outline.bounds,
                    outline.width,
                ));
            }
        }
        order.push(focused);
    }
    failures.push(format!(
        "{surface} at {interface} pt: Tab did not come round in {MAX_STOPS} stops"
    ));
    rings
}

/// Whether one of `rings` lies on `pane`'s own edge, as a pane's 1 px
/// keyboard ring does.
pub(crate) fn rings_edge(rings: &[Bounds<Pixels>], pane: Bounds<Pixels>) -> bool {
    rings.iter().any(|ring| {
        [
            (ring.left(), pane.left()),
            (ring.top(), pane.top()),
            (ring.right(), pane.right()),
            (ring.bottom(), pane.bottom()),
        ]
        .into_iter()
        .all(|(drawn, edge)| (drawn - edge).abs() <= px(0.5))
    })
}

/// Fails with every stop that failed.
pub(crate) fn assert_whole(failures: Vec<String>) {
    assert_whole_except(failures, &[]);
}

/// Fails with every stop that failed, except the known failures, each a
/// stop's place (the start of its failure, such as "Quick Open at 13 pt,
/// stop 1") and its finding (part of the failure's text), awaiting the
/// follow-up its caller names. A known failure that no longer happens fails
/// too, so its entry leaves with its fix.
pub(crate) fn assert_whole_except(failures: Vec<String>, known: &[(&str, &str)]) {
    let matches = |failure: &String, (at, finding): &(&str, &str)| {
        failure.starts_with(at) && failure.contains(finding)
    };
    let unexpected: Vec<_> = failures
        .iter()
        .filter(|failure| !known.iter().any(|entry| matches(failure, entry)))
        .cloned()
        .collect();
    let fixed: Vec<_> = known
        .iter()
        .filter(|entry| !failures.iter().any(|failure| matches(failure, entry)))
        .map(|(at, finding)| format!("{at}: {finding}"))
        .collect();
    assert!(
        unexpected.is_empty() && fixed.is_empty(),
        "stops that fail:\n{}\nknown failures that no longer happen; remove them:\n{}",
        unexpected.join("\n"),
        fixed.join("\n")
    );
}

/// The app's key bindings, Midnight, dialogs opening at rest and a 1280 ×
/// 800 window.
pub(crate) fn prepare(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        native_accessibility::bind_keys(cx);
        shortcuts::bind_keys(cx);
        cx.set_reduce_motion(true);
        app.update(cx, |app, cx| {
            app.settings.theme = ThemeSelection::BuiltIn(ThemeChoice::Midnight);
            app.settings.follow_system = false;
            app.apply_appearance(window, cx);
        })
    });
    cx.simulate_resize(size(px(1280.), px(800.)));
    draw(cx, 2);
}

/// Sets the interface size the next walk uses.
pub(crate) fn interface_size(cx: &mut VisualTestContext, interface: u8) {
    cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
    draw(cx, 2);
}

/// Closes `surface`, the open dialog, with Escape, as a person would, so it
/// closes through its own path; a dialog that Escape leaves open fails.
pub(crate) fn close_dialogs(
    cx: &mut VisualTestContext,
    surface: &str,
    interface: u8,
    failures: &mut Vec<String>,
) {
    for _ in 0..4 {
        if !cx.update(|window, cx| window.has_active_dialog(cx)) {
            return;
        }
        cx.simulate_keystrokes("escape");
        draw(cx, 2);
    }
    failures.push(format!(
        "{surface} at {interface} pt: Escape leaves it open"
    ));
    cx.update(|window, cx| window.close_all_dialogs(cx));
    draw(cx, 2);
}

/// What opening a dialog leaves running that a walk waits for: nothing, as
/// for most dialogs, whose reads go through the app's own tasks and its
/// operation executor, which [`open_dialog`] drains, or the reads a dialog
/// runs on a thread of its own, such as the Reflog's, which
/// `run_until_parked` cannot wait for.
pub(crate) trait Opened {
    /// Returns once what opening the dialog started has landed.
    fn finish(self, cx: &mut VisualTestContext);
}

impl Opened for () {
    fn finish(self, _: &mut VisualTestContext) {}
}

/// Opens a dialog with `open`, lets its reads finish, walks it, and closes
/// it.
async fn walk_dialog<O: Opened>(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    interface: u8,
    failures: &mut Vec<String>,
    open: impl FnOnce(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) -> O,
) {
    open_dialog(app, cx, surface, open).await;
    walk(app, cx, surface, interface, failures);
    close_dialogs(cx, surface, interface, failures);
}

/// Opens a dialog with `open` from History's list and lets the reads it
/// started land, so the dialog is walked as it stands once loaded rather
/// than while a reply can still change it; the layout grid (`layout_grid`)
/// opens its dialogs here too.
pub(crate) async fn open_dialog<O: Opened>(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    open: impl FnOnce(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) -> O,
) {
    // Each dialog opens from History's list, as from the page a person is
    // on, rather than from wherever the last dialog's close left focus.
    let list = app.read_with(cx, |app, _| app.focus.clone());
    cx.update(|window, cx| window.focus(&list, cx));
    draw(cx, 2);
    let opened = cx.update(|window, cx| app.update(cx, |app, cx| open(app, window, cx)));
    opened.finish(cx);
    // Tags opens only once its list, read on the app's operation executor,
    // lands. That executor answers on its own thread, which neither
    // `run_until_parked` nor `settle` waits for, so a slow read left the
    // dialog unopened when the walk began; once the executor drains, every
    // read the opener submitted has answered, and the replies land as the
    // app settles.
    app.read_with(cx, |app, _| app.operations.drain());
    views::tests::settle(app, cx).await;
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "{surface} opens"
    );
    assert!(
        !cx.update(|window, _| window.context_stack().is_empty()),
        "{surface} draws the control it focused"
    );
}

/// History and, with its file activated, Compare.
#[gpui::test]
async fn history_and_compare_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk(&app, cx, "History", interface, &mut failures);
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        views::tests::settle(&app, cx).await;
        walk(&app, cx, "Compare", interface, &mut failures);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        views::tests::settle(&app, cx).await;
    }
    assert_whole(failures);
}

/// History with the local watch unable to start: the status bar's Refresh
/// and Details… are Tab stops whose rings lie whole inside the bar, where at
/// 13 pt they ran 0.5 px past the window's foot.
#[gpui::test]
async fn the_refresh_warning_draws_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    crate::width_walk::unwatch(&app, cx);
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        let rings = walk(&app, cx, "The refresh warning", interface, &mut failures);
        for control in ["retry-local-refresh", "local-refresh-details"] {
            let drawn = cx.debug_bounds(control).expect(control);
            let ringed = rings.iter().any(|ring| {
                ring.left() < drawn.left()
                    && ring.right() > drawn.right()
                    && ring.top() < drawn.top()
                    && ring.bottom() > drawn.bottom()
            });
            assert!(
                ringed,
                "{control} at {interface} pt: a Tab stop with its ring"
            );
        }
    }
    assert_whole(failures);
}

/// Working Changes with a modified and an untracked file.
#[gpui::test]
async fn working_changes_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (fixture, app, cx) = views::tests::history_window(cx).await;
    let root = fixture.path().join("narrow");
    std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    views::tests::settle(&app, cx).await;
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk(&app, cx, "Working Changes", interface, &mut failures);
    }
    assert_whole(failures);
}

/// Settings and the theme editor it opens.
#[gpui::test]
async fn settings_and_the_theme_editor_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    cx.update(|_, cx| theme_editor::init(cx));
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    views::tests::settle(&app, cx).await;
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk(&app, cx, "Settings", interface, &mut failures);
        walk_dialog(
            &app,
            cx,
            "the theme editor",
            interface,
            &mut failures,
            |app, window, cx| app.open_theme_editor(None, window, cx),
        )
        .await;
    }
    assert_whole(failures);
}

/// The project hub, and History beside the project pane.
#[gpui::test]
async fn the_project_hub_and_pane_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
        views::tests::settle(&app, cx).await;
        walk(&app, cx, "the project hub", interface, &mut failures);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_history(window, cx);
                if !app.settings.project_pane {
                    app.toggle_project_pane(window, cx);
                }
            })
        });
        views::tests::settle(&app, cx).await;
        walk(
            &app,
            cx,
            "History beside the project pane",
            interface,
            &mut failures,
        );
    }
    assert_whole(failures);
}

/// The ring's room the project hub's list, Quick Open's form and Keyboard
/// Shortcuts keep inside their clips moves nothing, at 13 and 18 pt: laid
/// out without it, as a ring taking none lays out, every painted quad and
/// named element sits where it does with it, and the controls at the clips'
/// edges, the first project's Rename and Quick Open's scope buttons, lose
/// their rings. The walks above find the rings whole.
#[gpui::test]
async fn the_ring_room_at_list_and_dialog_edges_moves_nothing(cx: &mut TestAppContext) {
    use crate::tags::tests::assert_room_moves_nothing;
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    for interface in SIZES {
        interface_size(cx, interface);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
        views::tests::settle(&app, cx).await;
        // Projects opens with its selected action tab focused, and a ring of
        // no width paints none of that tab's ring quads; the room is
        // measured with focus on the window's root, where nothing draws one.
        let root = app.read_with(cx, |app, _| app.app_focus.clone());
        cx.update(|window, cx| window.focus(&root, cx));
        draw(cx, 2);
        assert_room_moves_nothing(
            cx,
            &app,
            &["rename-recent-project-0"],
            &["project-hub-recent"],
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_history(window, cx)));
        views::tests::settle(&app, cx).await;
        let mut failures = Vec::new();
        open_dialog(&app, cx, "Quick Open", |app, window, cx| {
            app.open_quick_file(window, cx)
        })
        .await;
        assert_room_moves_nothing(
            cx,
            &app,
            &["quick-open-worktree", "quick-open-revision"],
            &["quick-open-results"],
        );
        close_dialogs(cx, "Quick Open", interface, &mut failures);
        open_dialog(&app, cx, "Keyboard Shortcuts", |app, window, cx| {
            app.shortcut_help(window, cx)
        })
        .await;
        assert_room_moves_nothing(cx, &app, &[], &["shortcut-help-results"]);
        close_dialogs(cx, "Keyboard Shortcuts", interface, &mut failures);
        assert_whole(failures);
    }
}

/// The sizes a dialog opens at in [`dialogs_open_with_a_whole_ring_on_their_first_field`]:
/// the default size in a full window, and the largest in a half tile.
const OPENING: [(u8, f32, f32); 2] = [(13, 1280., 800.), (18, 461., 490.)];

/// The Reflog, the worktree manager and Tags each open with focus on their
/// first field, the filter, and its ring drawn whole in the first frames,
/// before any key, at 13 pt in 1280 × 800 and 18 pt in 461 × 490. The
/// worktree manager and Tags opened with focus on the dialog's shell, which
/// draws nothing, so the first Tab landed on Manage or on the filter; the
/// Reflog focused its filter below its content's fold at 18 pt in the half
/// tile, where nothing showed it.
#[gpui::test]
async fn dialogs_open_with_a_whole_ring_on_their_first_field(cx: &mut TestAppContext) {
    use std::{cell::RefCell, rc::Rc};
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    let mut failures = Vec::new();
    for (interface, width, height) in OPENING {
        cx.simulate_resize(size(px(width), px(height)));
        interface_size(cx, interface);
        for surface in ["the Reflog", "Worktrees", "Tags"] {
            let field = match surface {
                "the Reflog" => {
                    let browser = Rc::new(RefCell::new(None));
                    let opened = browser.clone();
                    open_dialog(&app, cx, surface, move |app, window, cx| {
                        let browser = app.open_reflog_browser(window, cx);
                        *opened.borrow_mut() = browser.clone();
                        browser
                    })
                    .await;
                    let browser = browser.borrow().clone().expect("the Reflog opens");
                    browser.read_with(cx, |browser, cx| browser.filter_focus(cx))
                }
                "Worktrees" => {
                    open_dialog(&app, cx, surface, |app, window, cx| {
                        app.open_worktree_manager(window, cx)
                    })
                    .await;
                    worktrees::settle_manager(&app, cx).await;
                    worktrees::first_field(&app, cx)
                }
                _ => {
                    open_dialog(&app, cx, surface, |app, window, cx| {
                        app.open_tags(window, cx)
                    })
                    .await;
                    tags::filter(&app, cx)
                }
            };
            // The pointer moving keeps the ring, as at every stop.
            cx.simulate_mouse_move(point(px(2.), px(2.)), None, Modifiers::none());
            draw(cx, 1);
            let at = format!("{surface} at {interface} pt in {width} × {height}");
            if !cx.update(|window, _| field.is_focused(window)) {
                failures.push(format!("{at}: its first field does not hold focus"));
            } else {
                let ring = ring(cx, &field, &mut |_, _| {});
                if ring.is_empty() {
                    failures.push(format!("{at}: its first field paints no ring"));
                }
                for outline in ring.iter().filter(|outline| !outline.whole) {
                    failures.push(format!(
                        "{at}: a clip cuts its first field's ring at {:?}, sides {:?}",
                        outline.bounds, outline.sides
                    ));
                }
            }
            close_dialogs(cx, surface, interface, &mut failures);
        }
    }
    assert_whole(failures);
}

/// Settings opens with focus on its first control, inside the window's
/// handlers, and its first frames draw that control's ring whole, scrolled
/// into view, at 13 pt in 1280 × 800 and 18 pt in 461 × 490, with no key
/// pressed, as when a click opens the page: Follow system appearance, or, on
/// Linux while the desktop's Omarchy theme decides the appearance and that
/// switch is unavailable, the Omarchy theme's card, whose ring otherwise
/// shows only after a key. It opened with focus on the window's root, which
/// draws nothing, so nothing showed where the keyboard stood until a Tab
/// landed on the tab strip; at 18 pt in the half tile the switch lies below
/// the page's fold.
#[gpui::test]
async fn settings_opens_with_a_whole_ring_on_its_first_control(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut desktops = vec![false];
    #[cfg(target_os = "linux")]
    let desktop = {
        desktops.push(true);
        tempfile::tempdir().unwrap()
    };
    let mut failures = Vec::new();
    for omarchy in desktops {
        #[cfg(target_os = "linux")]
        if omarchy {
            let root = desktop.path().join("current");
            std::fs::create_dir_all(root.join("theme")).unwrap();
            let colors = include_bytes!("../../tests/fixtures/omarchy/tokyo-night.toml");
            std::fs::write(root.join("theme/colors.toml"), colors).unwrap();
            std::fs::write(root.join("theme.name"), "tokyo-night\n").unwrap();
            cx.update(|_, cx| drop(appearance::omarchy::install(Some(root), cx)));
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.settings.theme = ThemeSelection::Omarchy;
                    app.apply_appearance(window, cx);
                })
            });
            views::tests::settle(&app, cx).await;
        }
        let control = if omarchy {
            "the Omarchy theme's card"
        } else {
            "Follow system appearance"
        };
        for (interface, width, height) in OPENING {
            cx.simulate_resize(size(px(width), px(height)));
            interface_size(cx, interface);
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
            draw(cx, 2);
            let at = format!("Settings at {interface} pt in {width} × {height}, {control}");
            let first = app.update(cx, |app, cx| {
                if omarchy {
                    app.theme_card_focus(ThemeSelection::Omarchy).clone()
                } else {
                    app.page_focus("follow-system", cx)
                }
            });
            let root = app.read_with(cx, |app, _| app.app_focus.clone());
            let (holds, inside) = cx
                .update(|window, cx| (first.is_focused(window), root.contains_focused(window, cx)));
            if !holds || !inside {
                failures.push(format!(
                    "{at}: it does not hold focus inside the window's handlers"
                ));
            } else {
                let ring = ring(cx, &first, &mut |_, _| {});
                if ring.is_empty() {
                    failures.push(format!("{at}: it paints no ring in view"));
                }
                for outline in ring.iter().filter(|outline| !outline.whole) {
                    failures.push(format!(
                        "{at}: a clip cuts its ring at {:?}, sides {:?}",
                        outline.bounds, outline.sides
                    ));
                }
            }
            cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
            views::tests::settle(&app, cx).await;
        }
    }
    assert_whole(failures);
}

/// The repository's dialogs: Reflog, interactive rebase, the rewrite review,
/// the command palette, Quick Open and Keyboard Shortcuts.
#[gpui::test]
async fn repository_dialogs_draw_a_whole_ring_at_every_stop(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = views::tests::history_window(cx).await;
    prepare(&app, cx);
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk_dialog(
            &app,
            cx,
            "the Reflog",
            interface,
            &mut failures,
            |app, window, cx| app.open_reflog_browser(window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "interactive rebase",
            interface,
            &mut failures,
            |app, window, cx| app.open_interactive_rebase(None, window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "the rewrite review",
            interface,
            &mut failures,
            |app, window, cx| app.open_rewrite_review(window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "the command palette",
            interface,
            &mut failures,
            |app, window, cx| app.open_command_palette(window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "Quick Open",
            interface,
            &mut failures,
            |app, window, cx| app.open_quick_file(window, cx),
        )
        .await;
        walk_dialog(
            &app,
            cx,
            "Keyboard Shortcuts",
            interface,
            &mut failures,
            |app, window, cx| app.shortcut_help(window, cx),
        )
        .await;
    }
    assert_whole(failures);
}

/// Moves History's selection between its first two rows, as a reply landing
/// between two frames moves the selected row: a quiet refresh that lists new
/// commits above it, such as a stash's two.
fn move_selection(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.selected_commit = Some(usize::from(app.selected_commit == Some(0)));
            cx.notify();
        })
    });
}

/// Only focus makes a stop's ring, never a change on the page between the
/// frames the ring is read from. A quiet refresh that landed there, listing
/// a new stash's two commits above History's selected row, moved that row's
/// 2 px accent leading border, and the Stashes walk over History read the
/// border it left as a ring cut on three sides (13 pt, stop 3, in one seed
/// of twelve). Here History's selection moves between the frames at every
/// stop: once for good, as that reply moved it, in History's own walk and in
/// a dialog's over History, and away and back, as a page beneath a dialog
/// that drew differently while the window held no focus would. Every stop
/// stays whole.
#[gpui::test]
async fn a_page_changing_between_a_stops_frames_is_not_its_ring(cx: &mut TestAppContext) {
    let (_fixture, app, cx) =
        views::tests::history_window_from(cx, views::tests::history_script(3, 1)).await;
    prepare(&app, cx);
    let mut failures = Vec::new();
    for interface in SIZES {
        interface_size(cx, interface);
        walk_with(
            &app,
            cx,
            "History as a reply lands",
            interface,
            &mut failures,
            &mut |cx, refocus| {
                if !refocus {
                    move_selection(&app, cx)
                }
            },
        );
        for (surface, back) in [
            ("Keyboard Shortcuts over History as a reply lands", false),
            ("Keyboard Shortcuts over History that marks the blur", true),
        ] {
            open_dialog(&app, cx, surface, |app, window, cx| {
                app.shortcut_help(window, cx)
            })
            .await;
            walk_with(
                &app,
                cx,
                surface,
                interface,
                &mut failures,
                &mut |cx, refocus| {
                    if !refocus || back {
                        move_selection(&app, cx)
                    }
                },
            );
            close_dialogs(cx, surface, interface, &mut failures);
        }
    }
    assert_whole(failures);
}
