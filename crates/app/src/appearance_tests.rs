use super::custom::{CustomTheme, TokenKind, channel_distance, contrast, luminance};
use super::*;
use gpui_kit as gpui;
use gpui_kit::{Background, Hsla};

/// In every palette a dialog's backdrop dims the page until the dialog's
/// surface stands at least 1.05:1 lighter than every page surface beneath
/// it, with no more than 60% black and no more black than that takes;
/// where 60% cannot lift a near-black canvas that far, the dialog is at
/// least level with every dimmed surface wherever 60% allows that. With
/// the kit's 20% black, Midnight's dialog was darker than History's
/// dimmed panel (1.03:1 the other way) and 1.00:1 over the dimmed
/// subtle; there its edge keeps 3:1 against the dialog and the dimmed
/// page alike.
#[test]
fn every_dialog_stands_above_the_page_its_backdrop_dims() {
    /// The least lift of `canvas` over the page surfaces dimmed by
    /// `alpha`, below 1 where one dimmed surface is lighter.
    fn lift(palette: Palette, alpha: f64) -> f64 {
        [palette.canvas, palette.panel, palette.subtle]
            .map(|surface| {
                let dimmed = backdrop_over(surface, alpha);
                let ratio = contrast(palette.canvas, dimmed);
                if luminance(palette.canvas) >= luminance(dimmed) {
                    ratio
                } else {
                    1. / ratio
                }
            })
            .into_iter()
            .fold(f64::INFINITY, f64::min)
    }
    let mut lifted = 0;
    let mut failures = Vec::new();
    for (name, palette) in test_palettes() {
        let alpha = palette.dialog_backdrop();
        let kit = if palette.is_light() { 0.05 } else { 0.2 };
        if !(kit..=f64::from(DIALOG_BACKDROP_MAX_PERCENT) / 100.).contains(&alpha) {
            failures.push(format!("{name}: backdrop {alpha}"));
        }
        let most = lift(palette, f64::from(DIALOG_BACKDROP_MAX_PERCENT) / 100.);
        // Where even 60% cannot leave the dialog level with the dimmed
        // page, as over a black canvas, more black would not help: the
        // kit's own dim stays.
        if most < 1. {
            if (alpha - kit).abs() > 1e-9 {
                failures.push(format!("{name}: {alpha} where no dim helps"));
            }
            continue;
        }
        let floor = if most >= DIALOG_LIFT { DIALOG_LIFT } else { 1. };
        let reached = lift(palette, alpha);
        if reached < floor {
            failures.push(format!("{name}: {reached:.3}:1 at {alpha}, needs {floor}"));
        }
        if alpha > kit + 0.005 && lift(palette, alpha - 0.01) >= floor {
            failures.push(format!("{name}: {alpha} where {} lifts it", alpha - 0.01));
        }
        lifted += usize::from(floor == DIALOG_LIFT);
    }
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(lifted >= 20, "the built-in palettes lift: {lifted}");

    let midnight = ThemeChoice::Midnight.palette();
    let alpha = midnight.dialog_backdrop();
    assert!(
        alpha > 0.2,
        "Midnight's backdrop {alpha} darkens past the kit's"
    );
    for surface in [midnight.canvas, midnight.panel, midnight.subtle] {
        let dimmed = backdrop_over(surface, alpha);
        let edge = contrast(midnight.border, dimmed);
        assert!(
            edge >= POPUP_EDGE,
            "Midnight's edge over #{dimmed:06x}: {edge:.3}:1"
        );
    }
    assert!(contrast(midnight.border, midnight.canvas) >= POPUP_EDGE);
}

/// Applying a palette installs its backdrop as the kit's overlay, which
/// every dialog draws beneath itself.
#[gpui::test]
fn the_applied_overlay_is_the_palettes_backdrop(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for choice in [ThemeChoice::Midnight, ThemeChoice::Daylight] {
            choice.apply(None, cx);
            let overlay = Theme::global(cx).colors.overlay;
            let alpha = choice.palette().dialog_backdrop() as f32;
            assert!(
                overlay.l == 0. && (overlay.a - alpha).abs() < 1e-6,
                "{choice:?}: overlay {overlay:?}, backdrop {alpha}"
            );
        }
    });
}

/// A probe that opens a bare kit dialog holding `dialog-edge-probe`.
struct DialogProbe;

impl gpui::Render for DialogProbe {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui_kit::Styled as _;
        gpui::div().size_full()
    }
}

/// A window holding [`DialogProbe`] with its dialog open.
pub(crate) fn dialog_probe(cx: &mut gpui::TestAppContext) -> &mut gpui::VisualTestContext {
    use gpui_kit::component::{Root, WindowExt as _};
    use gpui_kit::{AppContext as _, InteractiveElement as _, ParentElement as _, Styled as _};
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let probe = cx.new(|_| DialogProbe);
        Root::new(probe, window, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(600.)));
    cx.update(|window, cx| {
        window.open_dialog(cx, |dialog, _, _| {
            dialog.title("Probe").child(
                gpui::div()
                    .debug_selector(|| "dialog-edge-probe".into())
                    .h(gpui::px(40.)),
            )
        })
    });
    crate::tags::tests::draw(cx);
    cx.executor()
        .advance_clock(*gpui_kit::component::dialog::ANIMATION_DURATION);
    crate::tags::tests::draw(cx);
    cx
}

/// The border colours of the dialog panel the last frame painted: the
/// smallest quad with a 1 px border on every side around the probe's
/// content, each colour once.
pub(crate) fn painted_dialog_edge(cx: &mut gpui::VisualTestContext) -> Vec<Hsla> {
    let content = cx
        .debug_bounds("dialog-edge-probe")
        .expect("the dialog's content");
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |value: gpui_kit::ScaledPixels| gpui::px(value.as_f32() / scale);
        let quads = window.painted_quads();
        let bordered = |quad: &&gpui_kit::Quad| {
            let widths = quad.border_widths;
            [widths.top, widths.right, widths.bottom, widths.left]
                .iter()
                .all(|width| logical(*width) == gpui::px(1.))
        };
        let holds = |quad: &&gpui_kit::Quad| {
            logical(quad.bounds.left()) < content.left()
                && logical(quad.bounds.top()) < content.top()
                && logical(quad.bounds.right()) > content.right()
                && logical(quad.bounds.bottom()) > content.bottom()
        };
        let area = |quad: &&gpui_kit::Quad| {
            logical(quad.bounds.size.width).as_f32() * logical(quad.bounds.size.height).as_f32()
        };
        let panel = quads
            .iter()
            .filter(bordered)
            .filter(holds)
            .min_by(|a, b| area(a).total_cmp(&area(b)))
            .expect("the dialog paints a bordered panel around its content")
            .bounds;
        let mut edges = Vec::new();
        for quad in quads
            .iter()
            .filter(|quad| quad.bounds == panel)
            .filter(bordered)
        {
            if !edges.contains(&quad.border_color) {
                edges.push(quad.border_color);
            }
        }
        edges
    })
}

/// A kit dialog draws its panel's 1 px edge in the theme's `dialog_edge`
/// where an application sets one, and in the shared `border` otherwise,
/// as upstream does. The page's `border` is fitted against the page, not
/// against a dialog over a dimmed page: 1.28:1 in Braden.
#[gpui::test]
fn a_dialog_draws_its_edge_in_the_themes_dialog_edge(cx: &mut gpui::TestAppContext) {
    let cx = dialog_probe(cx);
    let unset = cx.update(|_, cx| {
        let theme = Theme::global_mut(cx);
        theme.dialog_edge = None;
        theme.border
    });
    crate::tags::tests::draw(cx);
    assert_eq!(painted_dialog_edge(cx), vec![unset], "unset: the border");
    let edge = Hsla::from(rgb(0x7a3e9d));
    cx.update(|_, cx| Theme::global_mut(cx).dialog_edge = Some(edge));
    crate::tags::tests::draw(cx);
    assert_eq!(painted_dialog_edge(cx), vec![edge], "set: the dialog edge");
}

/// Enter on a Button Tab focused in a dialog's body presses that Button,
/// once, as the key goes down, and runs no default action: the ring
/// shows the control the keyboard acts on (`DESIGN.md`, "Keyboard").
/// A held Enter's repeats and its release press nothing more. In a
/// single-line field and on the default Button, Enter is the dialog's
/// default action, as before, and a multi-line field takes it as a new
/// line. Bound to the kit's Confirm, Enter on the body's Button ran the
/// default action and pressed nothing.
#[gpui::test]
fn enter_presses_the_focused_button_in_a_dialog(cx: &mut gpui::TestAppContext) {
    use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
    use gpui_kit::component::{Root, WindowExt as _};
    use gpui_kit::{AppContext as _, Focusable as _, KeyDownEvent, Keystroke, ParentElement as _};
    use std::{cell::Cell, rc::Rc};
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let probe = cx.new(|_| DialogProbe);
        Root::new(probe, window, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(600.)));
    let (presses, confirms) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let press = cx.update(|_, cx| cx.focus_handle());
    let (field, notes) = cx.update(|window, cx| {
        (
            cx.new(|cx| InputState::new(window, cx)),
            cx.new(|cx| TextareaState::new(window, cx)),
        )
    });
    cx.update(|window, cx| {
        let (presses, confirms) = (presses.clone(), confirms.clone());
        let (press, field, notes) = (press.clone(), field.clone(), notes.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (presses, confirms) = (presses.clone(), confirms.clone());
            dialog
                .title("Probe")
                .child(Textarea::new(&notes))
                .child(Input::new(&field))
                .child(
                    crate::button("probe-press", "Press", "", false)
                        .track_focus(&press)
                        .on_click(move |_, _, _| presses.set(presses.get() + 1)),
                )
                .footer(crate::tags::dialog_footer("OK", Some("Cancel")))
                .on_ok(move |_, _, _| {
                    confirms.set(confirms.get() + 1);
                    false
                })
        })
    });
    crate::tags::tests::draw(cx);
    let focused = |cx: &mut gpui::VisualTestContext, handle: &gpui::FocusHandle| {
        cx.update(|window, _| handle.is_focused(window))
    };
    let field_focus = cx.update(|_, cx| field.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&field_focus, cx));
    crate::tags::tests::draw(cx);
    cx.simulate_keystrokes("tab");
    crate::tags::tests::draw(cx);
    assert!(focused(cx, &press), "Tab moves from the field onto Press");

    cx.simulate_keystrokes("enter");
    assert_eq!(
        (presses.get(), confirms.get()),
        (1, 0),
        "Enter presses Press"
    );
    for _ in 0..2 {
        cx.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
            is_held: true,
            prefer_character_input: false,
        });
    }
    // Its release; a key-down before it is one more repeat, which
    // presses nothing under X11 too.
    let enter_up = |cx: &mut gpui::VisualTestContext| {
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        })
    };
    enter_up(cx);
    cx.simulate_keystrokes("enter");
    enter_up(cx);
    assert_eq!(
        (presses.get(), confirms.get()),
        (2, 0),
        "a held Enter's repeats press nothing; a fresh press presses once"
    );
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));

    cx.update(|window, cx| window.focus(&field_focus, cx));
    crate::tags::tests::draw(cx);
    cx.simulate_keystrokes("enter");
    enter_up(cx);
    assert_eq!(
        (presses.get(), confirms.get()),
        (2, 1),
        "in the field, the default"
    );

    let notes_focus = cx.update(|_, cx| notes.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&notes_focus, cx));
    crate::tags::tests::draw(cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        (presses.get(), confirms.get()),
        (2, 1),
        "in the notes, no default"
    );
    assert_eq!(cx.update(|_, cx| notes.read(cx).value().to_string()), "\n");

    // The default Button: the footer's last, after Cancel.
    cx.update(|window, cx| window.focus(&press, cx));
    crate::tags::tests::draw(cx);
    cx.simulate_keystrokes("tab tab");
    crate::tags::tests::draw(cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(
        (presses.get(), confirms.get()),
        (2, 2),
        "on OK, the default"
    );
}

/// A window holding the dialog layer, 800 × 600, with nothing open.
fn enter_probe(cx: &mut gpui::TestAppContext) -> &mut gpui::VisualTestContext {
    use gpui_kit::AppContext as _;
    use gpui_kit::component::Root;
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let probe = cx.new(|_| DialogProbe);
        Root::new(probe, window, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(600.)));
    cx
}

fn key_down(cx: &mut gpui::VisualTestContext, key: &str, is_held: bool) {
    cx.simulate_event(gpui_kit::KeyDownEvent {
        keystroke: gpui_kit::Keystroke::parse_with_os(key, crate::shortcuts::host_os()).unwrap(),
        is_held,
        prefer_character_input: false,
    });
    crate::tags::tests::draw(cx);
}

fn key_up(cx: &mut gpui::VisualTestContext, key: &str) {
    cx.simulate_event(gpui_kit::KeyUpEvent {
        keystroke: gpui_kit::Keystroke::parse_with_os(key, crate::shortcuts::host_os()).unwrap(),
    });
    crate::tags::tests::draw(cx);
}

/// Opens dialog A, its focused body Button Delete… opening confirmation
/// B, whose default action counts a write: over A where `nested`, else
/// once A has closed, as Git profiles' Delete… opens Delete saved
/// profile. Returns the writes B ran.
fn open_delete(
    cx: &mut gpui::VisualTestContext,
    nested: bool,
) -> std::rc::Rc<std::cell::Cell<usize>> {
    use gpui_kit::ParentElement as _;
    use gpui_kit::component::WindowExt as _;
    let writes = std::rc::Rc::new(std::cell::Cell::new(0));
    let press = cx.update(|_, cx| cx.focus_handle());
    cx.update(|window, cx| {
        let (writes, press) = (writes.clone(), press.clone());
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let writes = writes.clone();
            dialog
                .title("A")
                .child(
                    crate::button("delete", "Delete…", "", false)
                        .track_focus(&press)
                        .on_click(move |_, window, cx| {
                            if !nested {
                                window.close_dialog(cx);
                            }
                            let writes = writes.clone();
                            window.open_alert_dialog(cx, move |dialog, _, _| {
                                let writes = writes.clone();
                                dialog
                                    .title("B")
                                    .footer(crate::tags::dialog_footer(
                                        "Delete profile",
                                        Some("Cancel"),
                                    ))
                                    .on_ok(move |_, _, _| {
                                        writes.set(writes.get() + 1);
                                        true
                                    })
                            });
                        }),
                )
                .footer(crate::tags::dialog_footer("Close", None))
        })
    });
    crate::tags::tests::draw(cx);
    cx.update(|window, cx| window.focus(&press, cx));
    crate::tags::tests::draw(cx);
    writes
}

/// Enter held on a body Button that opens a confirmation, over its
/// dialog (`nested`) or in its place, runs no write: the press opens it,
/// and the held key's repeats reach a dialog that waits for that press
/// to come up and confirms only on a fresh one. A released press leaves
/// it open, and a fresh Enter then confirms it. The dialog's `Confirm`
/// binding took every repeat, so one press ran the write.
fn held_enter_on_a_button_that_opens_a_confirmation(cx: &mut gpui::TestAppContext, nested: bool) {
    use gpui_kit::component::WindowExt as _;
    let cx = enter_probe(cx);
    let writes = open_delete(cx, nested);
    key_down(cx, "enter", false);
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    key_down(cx, "enter", true);
    key_down(cx, "enter", true);
    key_up(cx, "enter");
    assert_eq!(
        writes.get(),
        0,
        "nested {nested}: a held Enter ran no write"
    );
    key_down(cx, "enter", false);
    key_up(cx, "enter");
    assert_eq!(writes.get(), 1, "nested {nested}: a fresh Enter confirms");
}

#[gpui::test]
fn a_held_enter_never_confirms_what_it_opened_over_its_dialog(cx: &mut gpui::TestAppContext) {
    held_enter_on_a_button_that_opens_a_confirmation(cx, true);
}

#[gpui::test]
fn a_held_enter_never_confirms_what_it_opened_in_its_dialogs_place(cx: &mut gpui::TestAppContext) {
    held_enter_on_a_button_that_opens_a_confirmation(cx, false);
}

/// Under X11 a held key's repeats arrive as fresh presses with no
/// release between them: a second Enter going down before the first
/// comes up confirms nothing in the confirmation the first opened.
#[gpui::test]
fn a_repeat_that_looks_fresh_waits_for_the_enter_that_opened_the_dialog(
    cx: &mut gpui::TestAppContext,
) {
    let cx = enter_probe(cx);
    let writes = open_delete(cx, true);
    key_down(cx, "enter", false);
    key_down(cx, "enter", false);
    key_down(cx, "enter", false);
    assert_eq!(
        writes.get(),
        0,
        "no confirm before the opening Enter comes up"
    );
    key_up(cx, "enter");
    key_down(cx, "enter", false);
    assert_eq!(writes.get(), 1, "a fresh press after it confirms");
}

/// A dialog opened with no Enter down takes its first Enter, once, as
/// the key goes down; the held key's repeats confirm nothing more, and
/// Space on a body Button acts as it comes up, opening nothing early.
#[gpui::test]
fn a_dialog_opened_with_no_enter_down_confirms_on_the_first_press(cx: &mut gpui::TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let cx = enter_probe(cx);
    let confirms = std::rc::Rc::new(std::cell::Cell::new(0));
    cx.update(|window, cx| {
        let confirms = confirms.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let confirms = confirms.clone();
            dialog
                .title("Opened by a click")
                .footer(crate::tags::dialog_footer("OK", Some("Cancel")))
                .on_ok(move |_, _, _| {
                    confirms.set(confirms.get() + 1);
                    false
                })
        })
    });
    crate::tags::tests::draw(cx);
    key_down(cx, "enter", false);
    assert_eq!(confirms.get(), 1, "the first Enter confirms");
    key_down(cx, "enter", true);
    key_down(cx, "enter", true);
    key_up(cx, "enter");
    assert_eq!(confirms.get(), 1, "a held Enter's repeats confirm nothing");
    cx.update(|window, cx| window.close_all_dialogs(cx));
    crate::tags::tests::draw(cx);

    let writes = open_delete(cx, true);
    key_down(cx, "space", false);
    key_down(cx, "space", true);
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "A stays open"
    );
    key_up(cx, "space");
    assert_eq!(
        writes.get(),
        0,
        "Space opened the confirmation and ran nothing"
    );
}

/// In every palette a dialog's edge holds 3:1 against the dialog's
/// surface and against every page surface its backdrop dims, keeping
/// `border` where that already does and otherwise taking the least step
/// toward the text side that does. Braden's `border` stood 1.28:1 off its
/// dialog, and the popup edge's 1.4:1 left its edge 1.41:1 off the dimmed
/// page. Every applied palette draws its dialog in that edge.
#[gpui::test]
fn every_dialog_edge_holds_against_the_dialog_and_the_dimmed_page(cx: &mut gpui::TestAppContext) {
    let mut failures = Vec::new();
    for (name, palette) in test_palettes() {
        let edge = palette.dialog_edge();
        let alpha = palette.dialog_backdrop();
        let surfaces = [
            ("dialog", palette.canvas),
            ("dimmed canvas", backdrop_over(palette.canvas, alpha)),
            ("dimmed panel", backdrop_over(palette.panel, alpha)),
            ("dimmed subtle", backdrop_over(palette.subtle, alpha)),
        ];
        let least = |color: u32| {
            surfaces
                .iter()
                .map(|&(_, surface)| contrast(color, surface))
                .fold(f64::INFINITY, f64::min)
        };
        for (surface, color) in surfaces {
            let ratio = contrast(edge, color);
            if ratio < DIALOG_EDGE {
                failures.push(format!(
                    "{name}: #{edge:06x} on the {surface} #{color:06x} {ratio:.3}:1"
                ));
            }
        }
        if least(palette.border) >= DIALOG_EDGE {
            if edge != palette.border {
                failures.push(format!("{name}: #{edge:06x} where the border clears"));
            }
        } else {
            // The edge is a step from the border toward the text side,
            // and the step before it, one 256th nearer the border,
            // misses the rule: it moves no further than it must.
            let extreme = if palette.is_light() {
                0x000000
            } else {
                0xffffff
            };
            let nth = |n: u32| {
                [16, 8, 0].into_iter().fold(0, |color, shift| {
                    let from = (palette.border >> shift) & 0xff;
                    let to = (extreme >> shift) & 0xff;
                    color | (((from * (256 - n) + to * n) / 256) << shift)
                })
            };
            match (1..=256).find(|&n| nth(n) == edge) {
                Some(n) if least(nth(n - 1)) < DIALOG_EDGE => {}
                _ => failures.push(format!("{name}: #{edge:06x} is not the least step")),
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    let braden = ThemeChoice::Daylight.palette();
    assert_ne!(braden.dialog_edge(), braden.border, "Braden's edge moves");

    let cx = dialog_probe(cx);
    for (name, palette) in test_palettes() {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        crate::tags::tests::draw(cx);
        assert_eq!(
            painted_dialog_edge(cx),
            vec![Hsla::from(rgb(palette.dialog_edge()))],
            "{name}: the dialog draws its edge"
        );
    }
}

/// A dialog's bordered Cancel (`tags::dialog_cancel`) draws the dialog's
/// edge at rest, hovered and pressed in every palette, 3:1 or more
/// against the dialog around it. In the page's `border` it stood 1.28:1
/// off Braden's dialog, and the kit sets a secondary Button's edge from
/// its theme in each of those states, over any colour its caller gives.
#[gpui::test]
fn a_dialogs_cancel_draws_the_dialog_edge_in_every_state(cx: &mut gpui::TestAppContext) {
    use gpui_kit::component::{Root, WindowExt as _};
    use gpui_kit::{
        AppContext as _, InteractiveElement as _, Modifiers, MouseButton, ParentElement as _, point,
    };
    cx.update(gpui_kit::init);
    // The dialog otherwise slides in on the wall clock, its Cancel
    // moving between the bounds read and the press.
    cx.update(|cx| cx.set_reduce_motion(true));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let probe = cx.new(|_| DialogProbe);
        Root::new(probe, window, cx)
    });
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(600.)));
    cx.update(|window, cx| {
        window.open_dialog(cx, |dialog, _, _| {
            dialog.title("Probe").child(
                crate::tags::dialog_cancel("probe-cancel", "Cancel")
                    .debug_selector(|| "probe-cancel".into()),
            )
        })
    });
    let mut failures = Vec::new();
    for (name, palette) in test_palettes() {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        crate::tags::tests::draw(cx);
        let edge = palette.dialog_edge();
        let ratio = contrast(edge, palette.canvas);
        if ratio < DIALOG_EDGE {
            failures.push(format!("{name}: #{edge:06x} {ratio:.3}:1 on the dialog"));
        }
        let center = cx
            .debug_bounds("probe-cancel")
            .expect("the Cancel")
            .center();
        let painted = |cx: &mut gpui::VisualTestContext| {
            crate::tags::tests::draw(cx);
            crate::views::tests::painted_edges(cx, "probe-cancel")
        };
        let rest = painted(cx);
        cx.simulate_mouse_move(center, None, Modifiers::default());
        let hovered = painted(cx);
        cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::default());
        let pressed = painted(cx);
        cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::default());
        let away = point(gpui::px(-1.), gpui::px(-1.));
        cx.simulate_mouse_move(away, None, Modifiers::default());
        for (state, edges) in [
            ("at rest", rest),
            ("hovered", hovered),
            ("pressed", pressed),
        ] {
            if edges != vec![Hsla::from(rgb(edge))] {
                failures.push(format!("{name} {state}: {edges:?}, not #{edge:06x}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// In every palette, light and dark, a scrollbar's thumb holds 2:1
/// against each surface its track lies on, the canvas, the panel,
/// `subtle` and the raised surface the pickers' tracks rest on, as fitted
/// and as drawn a channel step nearer that surface, keeping `border`
/// where that already does and otherwise taking the least step toward
/// the text side that does, and stays quieter than secondary text there.
/// In `border` it stood 1.52:1 off Midnight's canvas, 1.39:1 off its
/// panel and 1.28:1 off Braden's canvas; fitted without the drawn step,
/// Midnight's #444f5f drew #434e5e on Keyboard Shortcuts' raised track,
/// 1.98:1.
#[test]
fn every_scrollbar_thumb_reads_two_to_one_and_quieter_than_secondary_text() {
    let mut failures = Vec::new();
    let mut moved = 0;
    let (mut light, mut dark) = (0, 0);
    for (name, palette) in test_palettes() {
        if palette.is_light() {
            light += 1;
        } else {
            dark += 1;
        }
        let thumb = palette.scrollbar_thumb();
        let surfaces = [
            ("canvas", palette.canvas),
            ("panel", palette.panel),
            ("subtle", palette.subtle),
            ("raised", palette.raised()),
        ];
        // The least of the colour's contrasts, as fitted and as drawn a
        // channel step nearer each surface.
        let least = |color: u32| {
            surfaces
                .iter()
                .map(|&(_, surface)| {
                    contrast(color, surface).min(contrast(drawn_nearer(color, surface), surface))
                })
                .fold(f64::INFINITY, f64::min)
        };
        for (surface, color) in surfaces {
            let ratio = contrast(thumb, color);
            let drawn = contrast(drawn_nearer(thumb, color), color);
            let text = contrast(palette.muted, color);
            if ratio < SCROLL_THUMB || drawn < SCROLL_THUMB || ratio >= text {
                failures.push(format!(
                    "{name}: #{thumb:06x} {ratio:.3}:1 ({drawn:.3}:1 drawn) on the {surface}, \
                     secondary text {text:.3}:1"
                ));
            }
        }
        if least(palette.border) >= SCROLL_THUMB {
            if thumb != palette.border {
                failures.push(format!("{name}: #{thumb:06x} where the border clears"));
            }
            continue;
        }
        moved += 1;
        // The thumb is a step from the border toward the text side, and
        // the step before it, one 256th nearer the border, misses.
        let extreme = if palette.is_light() {
            0x000000
        } else {
            0xffffff
        };
        let nth = |n: u32| {
            [16, 8, 0].into_iter().fold(0, |color, shift| {
                let from = (palette.border >> shift) & 0xff;
                let to = (extreme >> shift) & 0xff;
                color | (((from * (256 - n) + to * n) / 256) << shift)
            })
        };
        match (1..=256).find(|&n| nth(n) == thumb) {
            Some(n) if least(nth(n - 1)) < SCROLL_THUMB => {}
            _ => failures.push(format!("{name}: #{thumb:06x} is not the least step")),
        }
    }
    assert!(moved > 0, "some palette's thumb moves off its border");
    assert!(
        light > 0 && dark > 0,
        "light and dark palettes: {light}, {dark}"
    );
    let midnight = test_palettes()
        .into_iter()
        .find(|(name, _)| name == "Midnight")
        .expect("Midnight")
        .1;
    let thumb = midnight.scrollbar_thumb();
    assert!(
        contrast(drawn_nearer(thumb, midnight.raised()), midnight.raised()) >= SCROLL_THUMB,
        "Midnight's #{thumb:06x} holds 2:1 drawn on Keyboard Shortcuts' raised track"
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The thumb a scrollbar draws over rows (`Palette::over_rows_thumb`, the
/// Working body's) holds 2:1 by luminance, as fitted and as drawn a
/// channel step nearer, against the panel and a hovered, selected and
/// hovered selected row in every palette, and on the section headings'
/// `subtle`, and stays quieter than secondary text on the rows, which
/// its hovered state takes: the least step
/// from the palette's thumb toward the text, that thumb included. The
/// palette's thumb stood 1.46:1 on Midnight's selected row (#45505f on
/// #223b3b).
#[test]
fn the_thumb_over_rows_reads_two_to_one_on_every_row() {
    let mut failures = Vec::new();
    for (name, palette) in test_palettes() {
        let thumb = palette.over_rows_thumb();
        let rows = palette.thumb_rows();
        if Labels::fit(palette).over_rows_thumb != thumb {
            failures.push(format!("{name}: the labels keep another thumb"));
        }
        // The least of a colour's contrasts, as fitted and as drawn a
        // channel step nearer each row.
        let least = |color: u32| {
            rows.iter()
                .map(|&(_, row)| contrast(color, row).min(contrast(drawn_nearer(color, row), row)))
                .fold(f64::INFINITY, f64::min)
        };
        for (row, color) in rows {
            let ratio = contrast(thumb, color);
            let drawn = contrast(drawn_nearer(thumb, color), color);
            let text = contrast(palette.muted, color);
            if ratio < SCROLL_THUMB || drawn < SCROLL_THUMB || ratio >= text {
                failures.push(format!(
                    "{name}: #{thumb:06x} {ratio:.3}:1 ({drawn:.3}:1 drawn) on the {row}, \
                     secondary text {text:.3}:1"
                ));
            }
        }
        // The section headings' surface, which the palette's thumb is
        // fitted to, holds it too, as fitted and as drawn a channel step
        // nearer.
        let subtle = contrast(thumb, palette.subtle);
        let drawn = contrast(drawn_nearer(thumb, palette.subtle), palette.subtle);
        if subtle < SCROLL_THUMB || drawn < SCROLL_THUMB {
            failures.push(format!(
                "{name}: #{thumb:06x} {subtle:.3}:1 ({drawn:.3}:1 drawn) on the headings' \
                 subtle"
            ));
        }
        // A step from the palette's thumb toward the text, the thumb
        // itself or one whose step before it, one 256th nearer the
        // palette's thumb, misses.
        let base = palette.scrollbar_thumb();
        let nth = |n: u32| {
            [16, 8, 0].into_iter().fold(0, |color, shift| {
                let from = (base >> shift) & 0xff;
                let to = (palette.text >> shift) & 0xff;
                color | (((from * (256 - n) + to * n) / 256) << shift)
            })
        };
        match (0..=256).find(|&n| nth(n) == thumb) {
            Some(0) => {}
            Some(n) if least(nth(n - 1)) < SCROLL_THUMB => {}
            _ => failures.push(format!("{name}: #{thumb:06x} is not the least step")),
        }
    }
    let midnight = ThemeChoice::Midnight.palette();
    assert_ne!(
        midnight.over_rows_thumb(),
        midnight.scrollbar_thumb(),
        "Midnight's thumb moves off its selected row"
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The thumb of a scrollbar on a failed page's notice
/// (`Palette::removed_thumb`), whose track rests on the removed
/// background, holds 2:1 by luminance there, as fitted and as drawn a
/// channel step nearer, in every built-in and Omarchy palette, stays
/// quieter than secondary text there, which its hovered state takes, and
/// is the least step from the palette's thumb toward the text, that thumb
/// included; the labels hold it. The palette's thumb stood 1.66:1 on
/// Ember's removed background and 1.74:1 on Midnight's.
#[test]
fn the_removed_thumb_reads_two_to_one_on_the_removed_background() {
    let mut failures = Vec::new();
    for (name, palette) in test_palettes() {
        let thumb = palette.removed_thumb();
        let surface = palette.removed_background;
        if Labels::fit(palette).removed_thumb != thumb {
            failures.push(format!("{name}: the labels keep another thumb"));
        }
        // The least of a colour's contrasts, as fitted and as drawn a
        // channel step nearer the surface.
        let least = |color: u32| {
            contrast(color, surface).min(contrast(drawn_nearer(color, surface), surface))
        };
        let ratio = contrast(thumb, surface);
        let text = contrast(palette.muted, surface);
        if least(thumb) < SCROLL_THUMB || ratio >= text {
            failures.push(format!(
                "{name}: #{thumb:06x} {ratio:.3}:1 ({:.3}:1 drawn) on #{surface:06x}, \
                 secondary text {text:.3}:1",
                contrast(drawn_nearer(thumb, surface), surface)
            ));
        }
        let base = palette.scrollbar_thumb();
        let nth = |n: u32| {
            [16, 8, 0].into_iter().fold(0, |color, shift| {
                let from = (base >> shift) & 0xff;
                let to = (palette.text >> shift) & 0xff;
                color | (((from * (256 - n) + to * n) / 256) << shift)
            })
        };
        match (0..=256).find(|&n| nth(n) == thumb) {
            Some(0) => {}
            Some(n) if least(nth(n - 1)) < SCROLL_THUMB => {}
            _ => failures.push(format!("{name}: #{thumb:06x} is not the least step")),
        }
    }
    for choice in [ThemeChoice::Ember, ThemeChoice::Midnight] {
        let palette = choice.palette();
        assert_ne!(
            palette.removed_thumb(),
            palette.scrollbar_thumb(),
            "{}: the thumb moves off the removed background",
            choice.label()
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Where no step toward the text holds 2:1 on every row, as where the
/// text itself reads under 2:1 on a selected row, the thumb over rows
/// takes the least step from the palette's thumb toward white in a dark
/// palette and black in a light one that holds it, and never the text:
/// Midnight and Braden given their selected row's colour as text.
#[test]
fn the_thumb_over_rows_leaves_a_text_that_misses_for_the_extreme() {
    for choice in [ThemeChoice::Midnight, ThemeChoice::Daylight] {
        let mut palette = choice.palette();
        palette.text = palette.selected;
        let name = choice.label();
        let thumb = palette.over_rows_thumb();
        let rows = palette.thumb_rows();
        let least = |color: u32| {
            rows.iter()
                .map(|&(_, row)| contrast(color, row).min(contrast(drawn_nearer(color, row), row)))
                .fold(f64::INFINITY, f64::min)
        };
        assert!(
            least(palette.text) < SCROLL_THUMB,
            "{name}: the text misses on a row"
        );
        assert_ne!(thumb, palette.text, "{name}: the thumb is not the text");
        assert!(
            least(thumb) >= SCROLL_THUMB,
            "{name}: #{thumb:06x} holds 2:1 on every row"
        );
        let (base, extreme) = (
            palette.scrollbar_thumb(),
            if palette.is_light() {
                0x000000
            } else {
                0xffffff
            },
        );
        let nth = |n: u32| {
            [16, 8, 0].into_iter().fold(0, |color, shift| {
                let from = (base >> shift) & 0xff;
                let to = (extreme >> shift) & 0xff;
                color | (((from * (256 - n) + to * n) / 256) << shift)
            })
        };
        match (1..=256).find(|&n| nth(n) == thumb) {
            Some(n) if least(nth(n - 1)) < SCROLL_THUMB => {}
            _ => panic!("{name}: #{thumb:06x} is not the least step toward #{extreme:06x}"),
        }
    }
}

/// An application that has applied no palette fits its editor highlights as applying the
/// default palette would, with More contrast off and on: fitted with no palette as chosen
/// under More contrast, the fallback drew no changed-word rule and kept the current match
/// and the word tints as drawn off (Midnight's current match #284943 where an application
/// draws #004836).
#[gpui::test]
fn unapplied_highlights_fit_as_the_default_palette_applies(cx: &mut gpui::TestAppContext) {
    cx.update(gpui::init);
    for more in [false, true] {
        let (fallback, applied) = cx.update(|cx| {
            if cx.has_global::<GlobalEditorHighlights>() {
                cx.remove_global::<GlobalEditorHighlights>();
                cx.remove_global::<GlobalPalette>();
            }
            set_more_contrast(more, cx);
            let fallback = editor_highlights(cx);
            ThemeChoice::default().apply(None, cx);
            (fallback, editor_highlights(cx))
        });
        assert_eq!(fallback, applied, "More contrast {more}");
        assert_eq!(fallback.added_rule.is_some(), more, "More contrast {more}");
    }
}

/// Hovering an enabled unchecked Switch visibly lifts its track in every
/// palette: at least 6 of 255 in one channel from its resting `border`.
#[test]
fn every_palette_lifts_a_hovered_switch_track() {
    for (name, palette) in test_palettes() {
        let (rest, hover) = (palette.border, palette.switch_hover());
        let step = [16, 8, 0]
            .map(|shift| ((rest >> shift) & 0xff).abs_diff((hover >> shift) & 0xff))
            .into_iter()
            .max()
            .unwrap();
        assert!(step >= 6, "{name}: #{rest:06x} hovers to #{hover:06x}");
    }
}

#[test]
fn parallel_test_applications_keep_their_own_text_geometry() {
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        for (interface, code, desktop) in [(13, 12, 1.0f32), (18, 24, 1.5f32)] {
            let barrier = &barrier;
            scope.spawn(move || {
                store_text_sizes(interface, code);
                gitturtle_theme::store_desktop_text_scale(desktop);
                // Both applications have set their sizes before either
                // consumes geometry, exposing shared global storage reliably.
                barrier.wait();
                assert_eq!(ui_text(13.), px(f32::from(interface) * desktop));
                assert_eq!(code_text(), px(f32::from(code) * desktop));
            });
        }
    });
}

#[test]
fn desktop_text_scale_values_are_bounded_and_finite() {
    assert_eq!(desktop_text_scale_value(1.0), 1.0);
    assert_eq!(desktop_text_scale_value(1.25), 1.25);
    assert_eq!(desktop_text_scale_value(0.1), 0.5);
    assert_eq!(desktop_text_scale_value(12.0), 3.0);
    assert_eq!(desktop_text_scale_value(0.0), 1.0);
    assert_eq!(desktop_text_scale_value(-1.0), 1.0);
    assert_eq!(desktop_text_scale_value(f64::NAN), 1.0);
    assert_eq!(desktop_text_scale_value(f64::INFINITY), 1.0);
    // This test application begins with the default desktop factor.
    assert_eq!(desktop_text_scale(), 1.0);
}

#[test]
fn palettes_keep_text_and_diff_content_readable_in_each_theme() {
    for choice in ThemeChoice::ALL {
        let issues = choice.palette().readability_issues();
        assert!(
            issues.is_empty(),
            "{choice:?} breaks readability rules: {}",
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
}

/// `#rrggbbaa`, as the kit serializes a color, as `0xrrggbb`.
fn serialized(color: &serde_json::Value) -> u32 {
    let digits = color.as_str().expect("a serialized color");
    u32::from_str_radix(&digits[1..7], 16).expect("hex digits")
}

/// Every color the source and patch editors draw text in under a configured theme, by
/// syntax field in the toolkit's theme format: each field's color, and the editor
/// foreground, which a capture without a syntax color and unstyled text take. `None` for a
/// field without a color.
fn editor_text_colors(theme: &Theme) -> Vec<(String, Option<u32>)> {
    let syntax = serde_json::to_value(&theme.highlight_theme.style.syntax).unwrap();
    let mut colors = syntax
        .as_object()
        .unwrap()
        .iter()
        .map(|(field, style)| {
            let color = style.get("color").filter(|color| !color.is_null());
            (field.clone(), color.map(serialized))
        })
        .collect::<Vec<_>>();
    colors.push((
        "editor foreground".into(),
        Some(serialized(
            &serde_json::to_value(theme.colors.foreground).unwrap(),
        )),
    ));
    colors
}

/// Every syntax color the diff and source editors use, and the editor foreground, reads at
/// 4.5:1 on each background it is drawn on in each case's configured theme: the editor
/// background, where hunk header lines draw too, the active line, and the added and removed
/// line tints. Every field but emphasis has a color, so no capture falls back to the
/// toolkit's.
pub(crate) fn assert_syntax_colors_read(cases: impl IntoIterator<Item = (String, Palette, bool)>) {
    let mut below = Vec::new();
    let mut uncolored = Vec::new();
    let (mut palettes, mut measured) = (0, 0);
    for (name, palette, is_light) in cases {
        palettes += 1;
        let theme = palette.configured_theme(is_light);
        let style = &theme.highlight_theme.style;
        let background = |color: Option<Hsla>| {
            serialized(&serde_json::to_value(color.expect("a configured color")).unwrap())
        };
        let backgrounds = [
            ("editor background", background(style.editor_background)),
            ("active line", background(style.editor_active_line)),
            ("added lines", palette.added_background),
            ("removed lines", palette.removed_background),
        ];
        for (token, color) in editor_text_colors(&theme) {
            // Emphasis keeps its italic or weight and takes the color around it.
            let Some(color) = color else {
                if !token.starts_with("emphasis") {
                    uncolored.push(format!("{name}: {token}"));
                }
                continue;
            };
            for (surface, background) in backgrounds {
                measured += 1;
                let ratio = contrast(color, background);
                if ratio < 4.5 {
                    below.push(format!(
                        "{name}: {token} #{color:06x} on {surface} #{background:06x} {ratio:.2}:1"
                    ));
                }
            }
        }
    }
    assert!(
        below.is_empty(),
        "{} of {measured} pairs below 4.5:1:\n{}",
        below.len(),
        below.join("\n")
    );
    assert_eq!(uncolored, Vec::<String>::new());
    // Forty-four colored fields and the editor foreground, on four backgrounds.
    assert_eq!(measured, palettes * 45 * 4);
}

/// Text, every syntax color and the unified patch's diff colors read at 4.5:1 on each
/// highlight the editors draw behind them in each case's configured theme, and each
/// highlight stands [`HIGHLIGHT_DISTANCE`] (ΔEOK) from every line background it is drawn on:
/// the selection (the configured theme's text selection) and Find's other and current
/// matches on the editor background, the active line and both line tints; each word tint on
/// its own line tint, under text, the syntax colors but a diff file's removed lines
/// (`diff.minus`, [`SyntaxRoles::word_colors`]) and its diff color. Returns the cases
/// whose current Find match stands less than that distance from the other matches (its
/// accent underline still marks it).
pub(crate) fn assert_editor_highlights_read(
    cases: impl IntoIterator<Item = (String, Palette, bool)>,
) -> Vec<String> {
    let mut below = Vec::new();
    let mut close = Vec::new();
    let mut current_like_others = Vec::new();
    let mut palettes = 0;
    for (name, palette, is_light) in cases {
        palettes += 1;
        let theme = palette.configured_theme(is_light);
        let highlights = palette.editor_highlights();
        let selection = serialized(&serde_json::to_value(theme.colors.selection).unwrap());
        assert_eq!(selection, highlights.selection, "{name}: applied selection");
        // Rows and lists keep the palette's `selected`.
        let list_active = serialized(&serde_json::to_value(theme.colors.list_active).unwrap());
        assert_eq!(list_active, palette.selected, "{name}: list selection");
        let syntax = editor_text_colors(&theme)
            .into_iter()
            .filter_map(|(token, color)| color.map(|color| (token, color)))
            .collect::<Vec<_>>();
        let with = |extra: &[(&str, u32)]| {
            let mut drawn = syntax.clone();
            drawn.extend(
                extra
                    .iter()
                    .map(|&(token, color)| (token.to_owned(), color)),
            );
            drawn
        };
        let word = |extra: &[(&str, u32)]| {
            let mut drawn = with(extra);
            drawn.retain(|(token, _)| token != "diff.minus");
            drawn
        };
        let unified = with(&[
            ("unified added", palette.added),
            ("unified removed", palette.removed),
            ("unified hunk", palette.hunk),
        ]);
        let lines = [
            ("editor background", palette.canvas),
            ("active line", palette.panel),
            ("added lines", palette.added_background),
            ("removed lines", palette.removed_background),
        ];
        if oklab_distance(oklab(highlights.find_current), oklab(highlights.find_match))
            < HIGHLIGHT_DISTANCE
        {
            current_like_others.push(name.clone());
        }
        // Each highlight, its color, the colors drawn on it and the lines it sits on.
        type Case<'a> = (&'a str, u32, Vec<(String, u32)>, Vec<(&'a str, u32)>);
        let cases: [Case; 5] = [
            ("selection", selection, unified.clone(), lines.to_vec()),
            (
                "Find match",
                highlights.find_match,
                unified.clone(),
                lines.to_vec(),
            ),
            (
                "current Find match",
                highlights.find_current,
                unified,
                lines.to_vec(),
            ),
            (
                "added word tint",
                highlights.added_word,
                word(&[("unified added", palette.added)]),
                vec![lines[2]],
            ),
            (
                "removed word tint",
                highlights.removed_word,
                word(&[("unified removed", palette.removed)]),
                vec![lines[3]],
            ),
        ];
        for (highlight, background, drawn, lines) in cases {
            for (token, color) in drawn {
                let ratio = contrast(color, background);
                if ratio < 4.5 {
                    below.push(format!(
                        "{name}: {token} #{color:06x} on {highlight} #{background:06x} {ratio:.2}:1"
                    ));
                }
            }
            for (surface, line) in lines {
                let distance = oklab_distance(oklab(background), oklab(line));
                if distance < HIGHLIGHT_DISTANCE {
                    close.push(format!(
                        "{name}: {highlight} #{background:06x} on {surface} #{line:06x} ΔEOK {distance:.3}"
                    ));
                }
            }
        }
    }
    assert!(palettes > 0);
    assert!(
        below.is_empty(),
        "{} pairs below 4.5:1:\n{}",
        below.len(),
        below.join("\n")
    );
    assert!(
        close.is_empty(),
        "{} highlights within ΔEOK {HIGHLIGHT_DISTANCE} of their line:\n{}",
        close.len(),
        close.join("\n")
    );
    current_like_others
}

/// Every built-in, dark and light, and the faint-status custom theme below keep their syntax
/// readable on the editor highlights; `omarchy::palette::tests` measures the Omarchy
/// fixtures and arbitrary themes. On origin/main the selection and Find drew on `selected`,
/// where Alucard's `boolean` read at 3.83:1, and the word tints were a fixed mix, where
/// Dracula's `attribute` read at 2.59:1 on its added word tint.
#[test]
fn syntax_reads_on_every_editor_highlight() {
    let mut custom = CustomTheme::from_base(1, "Faint statuses", ThemeChoice::Porcelain);
    custom.palette.set(TokenKind::Renamed, 0x8b62c4);
    custom.palette.set(TokenKind::Modified, 0x9a6a1c);
    let like = assert_editor_highlights_read(
        ThemeChoice::ALL
            .into_iter()
            .map(|choice| (format!("{choice:?}"), choice.palette(), choice.is_light()))
            .chain([(custom.name.clone(), custom.palette, custom.is_light())]),
    );
    // Their accent underline marks the current match where its background cannot stand apart.
    assert_eq!(like, ["Sandstone"]);
}

/// The syntax colors of the twenty built-ins, and of a custom theme whose keyword and type
/// colors sit just above the 3:1 status rule it is held to, read on every editor
/// background; `omarchy::palette::tests` measures the Omarchy fixtures. On origin/main
/// these were the toolkit's default highlight theme's colors.
/// A ```diff fence in Markdown source draws its file headers and context lines in `text`
/// and its hunk,
/// removed and added lines in the fitted `hunk`, `removed` and `added` roles, in every
/// built-in and Omarchy palette, each at 4.5:1 or more on the canvas and the panel. On
/// origin/main a removed line took the keyword color (`renamed`), a hunk line the markup
/// color (`accent`), the headers keyword and string, and a context line the fence's code
/// color (`text.literal`, `added`). Markdown without a diff fence,
/// another language injected, draws exactly as before: no style it takes depends on the
/// diff fields.
#[test]
fn a_diff_fence_in_markdown_draws_the_patch_colours() {
    use gpui_kit::{
        combine_highlights,
        component::{Rope, highlighter::SyntaxHighlighter},
    };
    let fence = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,3 +1,3 @@ intro\n kept line\n-old line\n+new line\n";
    let document = format!("# Notes\n\nThe change:\n\n```diff\n{fence}```\n");
    let without =
        "# Notes\n\nSome `code` and **bold**.\n\n```rust\nfn main() { let s = \"x\"; }\n```\n";
    let styles = |text: &str, theme: &gpui_kit::component::highlighter::HighlightTheme| {
        let mut highlighter = SyntaxHighlighter::new("markdown");
        highlighter.update(None, &Rope::from(text), None);
        combine_highlights(Vec::new(), highlighter.styles(&(0..text.len()), theme))
            .collect::<Vec<_>>()
    };
    for (name, palette) in test_palettes() {
        let theme = palette.configured_theme(palette.is_light()).highlight_theme;
        let roles = palette.syntax_roles(None);
        let drawn = styles(&document, &theme);
        let mut start = document.find("```diff\n").unwrap() + "```diff\n".len();
        for line in fence.split_inclusive('\n') {
            let range = start..start + line.len() - 1;
            start += line.len();
            let (role, expected) =
                if line.starts_with("---") || line.starts_with("+++") || line.starts_with(' ') {
                    ("text", roles.text)
                } else if line.starts_with("@@") {
                    ("hunk", roles.function)
                } else if line.starts_with('-') {
                    ("removed", roles.removed)
                } else {
                    ("added", roles.string)
                };
            let colors: Vec<_> = drawn
                .iter()
                .filter(|(segment, _)| segment.start < range.end && segment.end > range.start)
                .map(|(_, style)| style.color)
                .collect();
            assert!(
                !colors.is_empty()
                    && colors
                        .iter()
                        .all(|color| *color == Some(rgb(expected).into())),
                "{name}: {line:?} draws {colors:?}, not its {role} color #{expected:06x}"
            );
            for surface in [palette.canvas, palette.panel] {
                let ratio = custom::contrast(expected, surface);
                assert!(
                    ratio >= 4.5,
                    "{name}: {role} #{expected:06x} reads {ratio:.2}:1 on #{surface:06x}"
                );
            }
        }
        let mut without_diff = (*theme).clone();
        let syntax = &mut without_diff.style.syntax;
        (
            syntax.diff_header,
            syntax.diff_hunk,
            syntax.diff_minus,
            syntax.diff_plus,
        ) = (None, None, None, None);
        assert_eq!(
            styles(without, &theme),
            styles(without, &without_diff),
            "{name}: Markdown without a diff fence takes a diff style"
        );
    }
}

/// A complete highlight, `update` without a timeout, keeps every layer it injects however
/// long one takes to parse. The toolkit stopped each injected parse after 20 ms of wall
/// time even then and dropped that layer, so Markdown's inline layer (code spans, emphasis,
/// links) went missing from a long paragraph, and from a short one whenever a loaded
/// machine paused the thread mid-parse: `a_diff_fence_in_markdown_draws_the_patch_colours`
/// once compared a pass without the inline layer with one that had it. This paragraph,
/// within the toolkit's 512 KiB bound on one injected language, parses for far longer.
#[test]
fn a_complete_markdown_highlight_keeps_inline_styles_however_long_they_parse() {
    use gpui_kit::{
        FontWeight,
        component::{Rope, highlighter::SyntaxHighlighter},
    };
    let document = format!(
        "{}The last **word**.\n",
        "Some `code` and **bold** text.\n".repeat(16_000)
    );
    let mut highlighter = SyntaxHighlighter::new("markdown");
    assert!(highlighter.update(None, &Rope::from(document.as_str()), None));
    let palette = ThemeChoice::Midnight.palette();
    let theme = palette.configured_theme(palette.is_light()).highlight_theme;
    let start = document.rfind("**word**").unwrap();
    let styles = highlighter.styles(&(start..start + "**word**".len()), theme.as_ref());
    assert!(
        styles
            .iter()
            .all(|(_, style)| style.font_weight == Some(FontWeight::BOLD)),
        "the last bold run draws {styles:?}"
    );
}

#[test]
fn syntax_colors_read_on_every_editor_background() {
    let mut custom = CustomTheme::from_base(1, "Faint statuses", ThemeChoice::Porcelain);
    custom.palette.set(TokenKind::Renamed, 0x8b62c4);
    custom.palette.set(TokenKind::Modified, 0x9a6a1c);
    assert_eq!(custom.palette.readability_issues(), Vec::new());
    assert_syntax_colors_read(
        ThemeChoice::ALL
            .into_iter()
            .map(|choice| (format!("{choice:?}"), choice.palette(), choice.is_light()))
            .chain([(custom.name.clone(), custom.palette, custom.is_light())]),
    );
}

/// Each syntax field draws in its role's palette color. Pinned on Tokyo Night with its
/// orange for `warning`, which Tokyo Night shares with `modified`: its nine role colors
/// are then distinct and each already reads on every editor background, so none is mixed
/// toward `text`.
#[test]
fn syntax_fields_take_their_palette_roles() {
    let mut custom = CustomTheme::from_base(1, "Tokyo orange", ThemeChoice::TokyoNight);
    custom.palette.set(TokenKind::Warning, 0xff9e64);
    let palette = custom.palette;
    assert_eq!(palette.readability_issues(), Vec::new());
    let roles: [(Option<u32>, &[&str]); 10] = [
        (Some(palette.renamed), &["keyword", "preproc"]),
        (
            Some(palette.added),
            &[
                "string",
                "string.escape",
                "string.regex",
                "string.special",
                "string.special.symbol",
                "text.literal",
                "text.code.span",
                "diff.plus",
            ],
        ),
        (Some(palette.removed), &["diff.minus"]),
        (Some(palette.warning), &["number", "boolean", "constant"]),
        (
            Some(palette.modified),
            &["type", "constructor", "enum", "variant"],
        ),
        (Some(palette.hunk), &["function", "diff.hunk"]),
        (
            Some(palette.accent),
            &[
                "tag",
                "tag.doctype",
                "attribute",
                "property",
                "link_text",
                "link_uri",
                "label",
                "title",
            ],
        ),
        (
            Some(palette.muted),
            &["comment", "comment_doc", "hint", "predictive"],
        ),
        (
            Some(palette.text),
            &[
                "variable",
                "variable.special",
                "embedded",
                "operator",
                "punctuation",
                "punctuation.bracket",
                "punctuation.delimiter",
                "punctuation.list_marker",
                "punctuation.special",
                "primary",
                "diff.context",
                "diff.header",
                "editor foreground",
            ],
        ),
        (None, &["emphasis", "emphasis.strong"]),
    ];
    let mut distinct = roles
        .iter()
        .filter_map(|(color, _)| *color)
        .collect::<Vec<_>>();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 9, "the role colors are distinct");

    let drawn = editor_text_colors(&palette.configured_theme(custom.is_light()));
    let mut wrong = Vec::new();
    for (field, color) in &drawn {
        let role = roles
            .iter()
            .find(|(_, fields)| fields.contains(&field.as_str()))
            .map(|&(role, _)| role);
        if role != Some(*color) {
            wrong.push(format!(
                "{field}: {}, wanted {}",
                color.map_or("none".into(), custom::format_hex),
                role.map_or("a role".into(), |role| role
                    .map_or("none".into(), custom::format_hex)),
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "fields off their roles:\n{}",
        wrong.join("\n")
    );
    // Every pinned field is a field the toolkit has.
    assert_eq!(
        drawn.len(),
        roles.iter().map(|(_, fields)| fields.len()).sum::<usize>()
    );
}

/// A custom theme whose `text` misses the text rule on one editor background draws all its
/// syntax in `text`, the color its readability warning names, rather than fitting each
/// role to a contrast `text` itself does not reach: Porcelain with a removed-line tint
/// `text` reads at 4.47:1 on.
#[test]
fn syntax_draws_in_text_where_text_misses_the_rule() {
    use custom::{ReadabilityBackground, ReadabilityForeground};
    let mut custom = CustomTheme::from_base(1, "Dim removals", ThemeChoice::Porcelain);
    custom.palette.set(TokenKind::RemovedBackground, 0xa88e98);
    let palette = custom.palette;
    let ratio = contrast(palette.text, palette.removed_background);
    assert!(
        (4.4..4.5).contains(&ratio),
        "text on removed lines {ratio:.3}:1"
    );
    assert!(palette.readability_issues().iter().any(|issue| {
        issue.foreground == ReadabilityForeground::Token(TokenKind::Text)
            && issue.background == ReadabilityBackground::Token(TokenKind::RemovedBackground)
    }));

    let drawn = editor_text_colors(&palette.configured_theme(custom.is_light()));
    let off_text = drawn
        .iter()
        .filter(|(field, color)| {
            let wanted = (!field.starts_with("emphasis")).then_some(palette.text);
            *color != wanted
        })
        .map(|(field, color)| format!("{field}: {:?}", color.map(custom::format_hex)))
        .collect::<Vec<_>>();
    assert!(
        off_text.is_empty(),
        "fields off text:\n{}",
        off_text.join("\n")
    );
    assert_eq!(drawn.len(), 47);
}

/// Tuned tokens clear their rule by this much: the rules are computed on declared colors,
/// and small text at 1x renders about 0.2 lower (DESIGN.md, semantic palette ownership).
const RASTERIZATION_MARGIN: f64 = 0.25;

#[test]
fn warning_messages_stay_readable_on_the_subtle_surface() {
    for choice in ThemeChoice::ALL {
        let palette = choice.palette();
        let ratio = contrast(palette.warning, palette.subtle);
        assert!(ratio >= 4.5, "{choice:?} warning on subtle {ratio:.3}:1");
    }
    // The five built-ins that were below 4.5:1 before the rule existed.
    for choice in [
        ThemeChoice::KanagawaLotus,
        ThemeChoice::RosePineDawn,
        ThemeChoice::OneLight,
        ThemeChoice::SolarizedDark,
        ThemeChoice::SolarizedLight,
    ] {
        let palette = choice.palette();
        let ratio = contrast(palette.warning, palette.subtle);
        assert!(
            ratio >= 4.5 + RASTERIZATION_MARGIN,
            "{choice:?} warning on subtle {ratio:.3}:1"
        );
    }
}

/// Kanagawa Lotus joined on 2026-11-05: its `selected` stepped darker for
/// the hover rule, and its secondary text with it, from 4.52:1 to 4.82:1
/// on the hovered selected row, its text darkened to stay above it.
#[test]
fn tuned_secondary_text_keeps_its_margin_on_hovered_selected_rows() {
    for choice in [
        ThemeChoice::SolarizedDark,
        ThemeChoice::SolarizedLight,
        ThemeChoice::OneDark,
        ThemeChoice::OneLight,
        ThemeChoice::RosePine,
        ThemeChoice::RosePineDawn,
        ThemeChoice::Dracula,
        ThemeChoice::KanagawaLotus,
    ] {
        let palette = choice.palette();
        let ratio = contrast(palette.muted, palette.row_hover(true));
        assert!(
            ratio >= 4.5 + RASTERIZATION_MARGIN,
            "{choice:?} muted on the hovered selected row {ratio:.3}:1"
        );
    }
}

/// Secondary text never reads at a higher contrast than body text on the
/// same surface, in every built-in palette and, on Linux, every Omarchy
/// fixture, with More contrast off and on: on a row at rest (the canvas,
/// panel, subtle and raised surfaces), hovered, selected and hovered
/// while selected, and on the added and removed line tints, each as the
/// mode draws it. Contrast is measured in luminance, so a shade that
/// differs from `text` by hue still reads above it wherever it lies
/// further from the surface in luminance. Under More contrast, Kanagawa
/// Lotus's secondary text walked toward `text` in sRGB to `#504F59`,
/// darker than its text `#4F4F5F`, and read above it on every surface
/// (4.894:1 against 4.871:1 on the hovered selected row), as One Dark's
/// `#B0B5BD` did beside its text `#AEB5C2` by a thousandth.
#[test]
fn secondary_text_never_reads_above_body_text() {
    let mut failures = Vec::new();
    for (name, chosen) in test_palettes() {
        for (mode, palette) in [
            ("off", chosen),
            ("More contrast", chosen.with_more_contrast()),
        ] {
            for (state, surface) in [
                ("rest on canvas", palette.canvas),
                ("rest on panel", palette.panel),
                ("rest on subtle", palette.subtle),
                ("rest on raised", palette.raised()),
                ("hovered", palette.row_hover(false)),
                ("selected", palette.selected),
                ("hovered selected", palette.row_hover(true)),
                ("added lines", palette.added_background),
                ("removed lines", palette.removed_background),
            ] {
                let text = contrast(palette.text, surface);
                let muted = contrast(palette.muted, surface);
                if muted > text {
                    failures.push(format!(
                        "{name} ({mode}) muted #{:06x} {muted:.3}:1 above text #{:06x} \
                         {text:.3}:1 on {state} #{surface:06x}",
                        palette.muted, palette.text
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn palette_lightness_matches_each_built_in_choice() {
    for choice in ThemeChoice::ALL {
        assert_eq!(choice.is_light(), choice.palette().is_light(), "{choice:?}");
    }
}

#[test]
fn theme_switch_updates_resolved_component_backgrounds_with_foregrounds() {
    // Reuse one theme to cover dark/light and dark/dark switches. Component
    // buttons read token backgrounds but legacy foreground colors, so a
    // palette-only assertion cannot catch a stale white primary button.
    // A custom palette, built from a base with two edited tokens, takes the same path.
    let mut edited = ThemeChoice::Nord.palette();
    edited.set(custom::TokenKind::Accent, 0x1f6f5c);
    edited.set(custom::TokenKind::Removed, 0xff9aa2);
    let custom = custom::CustomTheme {
        id: 1,
        name: "Edited Nord".into(),
        base: ThemeChoice::Nord,
        palette: edited,
    };
    let cases = ThemeChoice::ALL
        .into_iter()
        .map(|choice| (format!("{choice:?}"), choice.palette(), choice.is_light()))
        .chain([(custom.name.clone(), custom.palette, custom.is_light())]);
    let mut theme = Theme::default();
    for (choice, palette, is_light) in cases {
        let roles = palette.syntax_roles(None);
        configure_palette(
            palette,
            is_light,
            false,
            palette,
            roles,
            EditorHighlights::fit(palette, roles, None),
            &mut theme,
        );
        let foreground: Hsla = rgb(palette.accent_foreground).into();
        assert_eq!(theme.colors.button_primary_foreground, foreground);
        for token in [
            theme.tokens.button_danger,
            theme.tokens.button_danger_hover,
            theme.tokens.button_danger_active,
        ] {
            let foreground = u32::from(theme.colors.button_danger_foreground.to_rgb()) >> 8;
            let background = u32::from(token.color.to_rgb()) >> 8;
            assert!(
                contrast(foreground, background) >= 4.5,
                "{choice} destructive action label must remain readable"
            );
            assert_eq!(token.background, Background::from(token.color));
        }
        for (token, expected) in [
            (theme.tokens.button_primary, palette.accent),
            (theme.tokens.button_primary_hover, palette.accent_hover),
            (theme.tokens.button_primary_active, palette.accent_active),
            (theme.tokens.button_secondary, palette.subtle),
            (theme.tokens.button_secondary_hover, palette.hover),
            (theme.tokens.button_secondary_active, palette.selected),
            (theme.tokens.list_active, palette.selected),
            (theme.tokens.popover, palette.raised()),
            (theme.tokens.scrollbar_thumb, palette.scrollbar_thumb()),
        ] {
            let color: Hsla = rgb(expected).into();
            assert_eq!(token.color, color, "{choice} resolved color");
            assert_eq!(
                token.background,
                Background::from(color),
                "{choice} renderable background"
            );
        }
    }
}

/// The kit's ghost hover read 1.16:1 the wrong way over a hovered row. The
/// tightest built-in lift is Rosé Pine's 1.078:1 over its canvas, from a
/// hover that is only 1.087:1 on its own panel (DESIGN.md records that limit).
const CONTROL_HOVER_LIFT: f64 = 1.07;
/// Selected surfaces keep 1.15:1 from panels; the tightest pressed lift is
/// Kanagawa Wave's 1.128:1 over a hovered row.
const CONTROL_PRESS_LIFT: f64 = 1.12;
/// The kit's ghost pressed fill differed from its hover by (1, 2, 2). This
/// is the pressed-step readability rule's distance: Sandstone and Porcelain
/// were tuned past it (from 3 and 4 to 8), and Catppuccin Mocha and Nord
/// hold it exactly over their selected rows.
const CONTROL_PRESS_DISTANCE: u32 = custom::PRESSED_STEP;

/// Two Buttons a light palette presses: the kit's secondary (Fetch, Pull,
/// the branch and profile pickers), whose pressed fill is `selected` over
/// its hovered `hover`, and the shared helper, whose layers repeat that
/// step, each on the panel and on `subtle`.
struct PressTargets;

impl gpui::Render for PressTargets {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui_kit::component::button::{Button, ButtonVariants as _};
        use gpui_kit::{InteractiveElement as _, ParentElement as _, div};

        let p = palette(cx);
        div()
            .flex()
            .flex_col()
            .children(
                [("panel", p.panel), ("subtle", p.subtle)].map(|(surface, color)| {
                    div().flex().gap_4().p_4().bg(rgb(color)).children(
                        [("secondary", "Fetch"), ("control", "Options")].map(|(kind, label)| {
                            let button = Button::new(disabled_selector(kind, surface))
                                .debug_selector(move || format!("{kind}-{surface}"))
                                .label(label)
                                .on_click(|_, _, _| {});
                            if kind == "secondary" {
                                button.secondary()
                            } else {
                                button.with_variant(control_button_variant(false))
                            }
                        }),
                    )
                }),
            )
    }
}

/// Braden (`daylight`) and Rosé Pine Dawn press deeper than they hover.
/// Braden's pressed fill had the brightness of its hover and differed by
/// hue, 1.02:1 lighter over its selected row, and Dawn's stood exactly the
/// pressed step, 6 of 255 in one channel, from its hover. Now on each of
/// the six surfaces the shared button's pressed layer reads at least
/// 1.02:1 darker than its hover layer and at least 10 from it, four past
/// the step, and so does the kit's secondary, `selected` over `hover`, as
/// the scene paints both Buttons hovered and held down. Text and secondary
/// text keep 4.75:1 on the darker selected rows.
#[gpui::test]
fn braden_and_rose_pine_dawn_press_deeper_than_they_hover(cx: &mut gpui::TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton, point};

    const DEEPER: f64 = 1.02;
    const STEP: u32 = custom::PRESSED_STEP + 4;
    // How much darker `pressed` reads than `hovered`: below 1 when lighter.
    let deeper =
        |hovered: u32, pressed: u32| (luminance(hovered) + 0.05) / (luminance(pressed) + 0.05);
    let mut failures = Vec::new();
    for choice in [ThemeChoice::Daylight, ThemeChoice::RosePineDawn] {
        let palette = choice.palette();
        let (hover, pressed) = (
            palette.control_fill(palette.hover),
            palette.control_fill(palette.selected),
        );
        let surfaces = [
            palette.canvas,
            palette.panel,
            palette.subtle,
            palette.hover,
            palette.selected,
            palette.row_hover(true),
        ];
        let mut fills = surfaces
            .map(|surface| {
                (
                    composite_color(hover, surface),
                    composite_color(pressed, surface),
                )
            })
            .to_vec();
        fills.push((palette.hover, palette.selected));
        for (hovered, held) in fills {
            let (depth, step) = (deeper(hovered, held), channel_distance(hovered, held));
            if depth < DEEPER || step < STEP {
                failures.push(format!(
                    "{choice:?} pressed {held:06x} under hover {hovered:06x}: {depth:.3}:1, {step} apart"
                ));
            }
        }
        if palette.pressed_step() < STEP {
            failures.push(format!(
                "{choice:?} pressed step {}",
                palette.pressed_step()
            ));
        }
        for row in [palette.selected, palette.row_hover(true)] {
            for (what, color) in [("text", palette.text), ("muted", palette.muted)] {
                let ratio = contrast(color, row);
                if ratio < LABEL_RULE + LABEL_MARGIN {
                    failures.push(format!("{choice:?} {what} on {row:06x} {ratio:.3}:1"));
                }
            }
        }
    }

    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|_, _| PressTargets);
    for choice in [ThemeChoice::Daylight, ThemeChoice::RosePineDawn] {
        cx.update(|window, cx| choice.apply(Some(window), cx));
        let palette = choice.palette();
        for (surface, color) in [("panel", palette.panel), ("subtle", palette.subtle)] {
            for kind in ["secondary", "control"] {
                let selector = disabled_selector(kind, surface);
                let bounds = cx.debug_bounds(selector).expect("the button");
                let painted = |cx: &mut gpui_kit::VisualTestContext| {
                    cx.update(|window, cx| window.draw(cx).clear(cx));
                    let (fills, _) = painted_button(cx, bounds);
                    fills.iter().fold(color, |beneath, fill| {
                        composite(fill.as_solid().expect("a solid fill").to_rgb(), beneath)
                    })
                };
                cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
                let hovered = painted(cx);
                cx.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
                let held = painted(cx);
                cx.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
                cx.simulate_mouse_move(point(px(-1.), px(-1.)), None, Modifiers::default());
                let (depth, step) = (deeper(hovered, held), channel_distance(hovered, held));
                if hovered == color || depth < DEEPER || step < STEP {
                    failures.push(format!(
                        "{choice:?} {kind} on the {surface}: hovered {hovered:06x}, held {held:06x}, {depth:.3}:1, {step} apart"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The Omarchy fitter's rule (`omarchy::palette`'s `HOVER_STEP`) for the
/// built-ins too: a secondary control's hover stays distinguishable from its
/// resting `subtle` fill.
#[test]
fn every_built_in_hover_steps_off_subtle() {
    let short: Vec<_> = ThemeChoice::ALL
        .into_iter()
        .map(|choice| (choice, choice.palette()))
        .map(|(choice, p)| (choice, p.hover, p.subtle, contrast(p.hover, p.subtle)))
        .filter(|(.., ratio)| *ratio < 1.08)
        .collect();
    assert!(
        short.is_empty(),
        "hover too close to subtle: {}",
        short
            .iter()
            .map(|(choice, hover, subtle, ratio)| format!(
                "{choice:?} #{hover:06x} on #{subtle:06x} {ratio:.4}"
            ))
            .collect::<Vec<_>>()
            .join("; ")
    );
}

#[test]
fn shared_button_fills_lift_every_surface_and_keep_the_label_readable() {
    assert_eq!(ThemeChoice::ALL.len(), 20);
    for choice in ThemeChoice::ALL {
        let palette = choice.palette();
        let (hover, pressed) = (
            palette.control_fill(palette.hover),
            palette.control_fill(palette.selected),
        );
        let label = palette.control_label();
        // Only the two palettes whose `text` falls below the rule over a
        // fill move their label; every other palette keeps `text`.
        assert_eq!(
            label == palette.text,
            !matches!(choice, ThemeChoice::KanagawaLotus | ThemeChoice::OneDark),
            "{choice:?} label {label:06x}, text {:06x}",
            palette.text
        );
        for (layer, state) in [(hover, palette.hover), (pressed, palette.selected)] {
            let over_panel = composite_color(layer, palette.panel);
            assert!(
                channel_distance(over_panel, state) <= 1,
                "{choice:?} fill over panel {over_panel:06x}, not {state:06x}"
            );
        }
        // The filled color, and its contrast with the surface when it moves
        // the way hover moves in this theme (0 otherwise).
        let lift = |layer, beneath| {
            let filled = composite_color(layer, beneath);
            let lifted = if choice.is_light() {
                luminance(filled) < luminance(beneath)
            } else {
                luminance(filled) > luminance(beneath)
            };
            let ratio = if lifted {
                contrast(filled, beneath)
            } else {
                0.
            };
            (filled, ratio)
        };
        for (surface, beneath) in [
            ("panel", palette.panel),
            ("subtle", palette.subtle),
            ("canvas", palette.canvas),
            ("hovered row", palette.hover),
            ("selected row", palette.selected),
            ("hovered selected row", palette.row_hover(true)),
        ] {
            let (hovered, ratio) = lift(hover, beneath);
            assert!(
                ratio >= CONTROL_HOVER_LIFT,
                "{choice:?} hover {hovered:06x} over the {surface} {beneath:06x}: {ratio:.4}"
            );
            let (held, ratio) = lift(pressed, beneath);
            assert!(
                ratio >= CONTROL_PRESS_LIFT,
                "{choice:?} pressed {held:06x} over the {surface} {beneath:06x}: {ratio:.4}"
            );
            for (state, filled) in [("hover", hovered), ("pressed", held)] {
                // A moved label clears the rule with the margin as well.
                let ratio = contrast(label, filled);
                let margin = if label == palette.text {
                    0.
                } else {
                    RASTERIZATION_MARGIN
                };
                assert!(
                    ratio >= 4.5 + margin,
                    "{choice:?} label {label:06x} on the {state} fill {filled:06x} over \
                     the {surface}: {ratio:.3}"
                );
            }
            let distance = channel_distance(held, hovered);
            assert!(
                distance >= CONTROL_PRESS_DISTANCE,
                "{choice:?} pressed {held:06x} is {distance} from hover {hovered:06x} \
                 on the {surface}"
            );
        }
    }
}

#[test]
fn selected_shared_button_hover_stays_a_pressed_step_from_rest() {
    for choice in ThemeChoice::ALL {
        let palette = choice.palette();
        // Both fills are opaque, so the step is the same on every surface.
        let (resting, hovered) = (palette.selected, palette.row_hover(true));
        let distance = channel_distance(hovered, resting);
        assert!(
            distance >= CONTROL_PRESS_DISTANCE,
            "{choice:?} selected hover {hovered:06x} is {distance} from {resting:06x}"
        );
        let label = palette.control_label();
        let margin = if label == palette.text {
            0.
        } else {
            RASTERIZATION_MARGIN
        };
        let ratio = contrast(label, hovered);
        assert!(
            ratio >= 4.5 + margin,
            "{choice:?} label {label:06x} on the selected hover {hovered:06x}: {ratio:.3}"
        );
    }
}

/// The ring a focused Button draws, as the palette application installs it,
/// against the same pixel unfocused. Ring and gap lie outside the edge, on
/// the surface beneath, so the Button's fill never reaches them; only an
/// opacity a state sets on the whole Button dims the ring with it.
#[gpui::test]
fn focused_button_ring_clears_the_graphic_rule_on_every_surface_and_state(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        let mut failures = Vec::new();
        for choice in ThemeChoice::ALL {
            choice.apply(None, cx);
            let palette = choice.palette();
            let theme = Theme::global(cx);
            let (setting, ring) = (theme.button_focus_ring, theme.colors.ring.to_rgb());
            if setting.width < px(2.) || setting.gap < px(1.) {
                failures.push(format!(
                    "{choice:?} ring {:?} wide, {:?} outside the edge",
                    setting.width, setting.gap
                ));
            }
            // The kit fades no state the helper paints (only a loading
            // Button), so the helper's selected hover is the one to read.
            let selected_hover = control_selected_hover(StyleRefinement::default())
                .opacity
                .unwrap_or(1.);
            let states = [
                ("at rest", 1.),
                ("selected", 1.),
                ("selected and hovered", selected_hover),
                ("hovered", 1.),
                ("pressed", 1.),
                ("disabled", 1.),
            ];
            for (surface, beneath) in [
                ("panel", palette.panel),
                ("subtle", palette.subtle),
                ("canvas", palette.canvas),
                ("hovered row", palette.hover),
                ("selected row", palette.selected),
                ("hovered selected row", palette.row_hover(true)),
            ] {
                let below = states
                    .iter()
                    .filter_map(|&(state, opacity)| {
                        let layer = Rgba {
                            a: ring.a * setting.opacity * opacity,
                            ..ring
                        };
                        let ratio = contrast(composite(layer, beneath), beneath);
                        (ratio < custom::GRAPHIC).then(|| format!("{state} {ratio:.2}:1"))
                    })
                    .collect::<Vec<_>>();
                if !below.is_empty() {
                    failures.push(format!(
                        "{choice:?} on the {surface} {beneath:06x}: {}",
                        below.join(", ")
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "focused Button ring below {}:1 or not 2 px wide, 1 px outside the edge\n{}",
            custom::GRAPHIC,
            failures.join("\n")
        );
    });
}

/// Every focus ring a kit control draws, a text field's, a checkbox's and
/// a Switch's, read from the frame with the control focused: each quad in
/// the theme's ring colour around it, at the opacity it is painted with,
/// over each surface such a control rests on (the canvas a dialog shows,
/// the panel and `subtle`), reads 3:1 (WCAG 1.4.11), in every built-in
/// palette with More contrast off and on, with the ring the palette's
/// accent. All three draw the Button's ring, 2 px of the full accent 1 px
/// outside their edge, and each must paint a quad of the ring's width:
/// the field's own 1 px accent edge, in the same colour, is not its ring.
/// The field and the checkbox drew the kit's, 3 px at half opacity
/// directly outside it, which read under 3:1 in 16 of the 20 palettes:
/// 2.27:1 on Porcelain's canvas, 2.06:1 on Sandstone's `subtle`.
#[gpui::test]
fn every_palette_draws_kit_focus_rings_at_three_to_one(cx: &mut gpui::TestAppContext) {
    use gpui_kit::component::checkbox::Checkbox;
    use gpui_kit::component::input::{Input, InputState};
    use gpui_kit::component::switch::Switch;
    use gpui_kit::{
        AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _,
        InteractiveElement as _, IntoElement, ParentElement as _, Render, ScaledPixels,
        Styled as _, div, point,
    };

    struct Controls {
        field: Entity<InputState>,
        checkbox: FocusHandle,
        switch: FocusHandle,
    }
    impl Render for Controls {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .items_start()
                .gap_8()
                .p_8()
                .child(
                    div()
                        .debug_selector(|| "ring-field".into())
                        .w(px(160.))
                        .child(Input::new(&self.field)),
                )
                .child(
                    div().debug_selector(|| "ring-checkbox".into()).child(
                        Checkbox::new("ring-checkbox")
                            .track_focus(&self.checkbox)
                            .label("Include"),
                    ),
                )
                .child(
                    div()
                        .debug_selector(|| "ring-switch".into())
                        .child(Switch::new("ring-switch").track_focus(&self.switch)),
                )
        }
    }

    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|window, cx| Controls {
        field: cx.new(|cx| InputState::new(window, cx)),
        checkbox: cx.focus_handle(),
        switch: cx.focus_handle(),
    });
    let handles = cx.update(|_, cx| {
        let view = view.read(cx);
        [
            ("ring-field", view.field.read(cx).focus_handle(cx)),
            ("ring-checkbox", view.checkbox.clone()),
            ("ring-switch", view.switch.clone()),
        ]
    });
    let mut failures = Vec::new();
    for more in [false, true] {
        for choice in ThemeChoice::ALL {
            cx.update(|window, cx| {
                set_more_contrast(more, cx);
                choice.apply(Some(window), cx);
            });
            for (id, handle) in &handles {
                cx.update(|window, cx| window.focus(handle, cx));
                crate::tags::tests::draw(cx);
                let control = cx.debug_bounds(id).expect("the control");
                let (applied, ring, width, mut rings) = cx.update(|window, cx| {
                    let scale = window.scale_factor();
                    let logical = |value: ScaledPixels| px(value.as_f32() / scale);
                    let theme = Theme::global(cx);
                    let (ring, width) = (packed(theme.ring), theme.button_focus_ring.width);
                    let rings = window
                        .painted_quads()
                        .into_iter()
                        .filter(|quad| quad.border_widths.top.as_f32() > 0.)
                        .filter(|quad| {
                            packed(quad.border_color) == ring && quad.border_color.a > 0.
                        })
                        .filter(|quad| {
                            let drawn = Bounds::from_corners(
                                point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                                point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                            );
                            control.dilate(px(8.)).contains(&drawn.origin)
                        })
                        .map(|quad| (quad.border_color.to_rgb(), logical(quad.border_widths.top)))
                        .collect::<Vec<_>>();
                    (palette(cx), ring, width, rings)
                });
                // GPUI paints a border-only quad once per side.
                rings.dedup();
                if ring != applied.accent {
                    failures.push(format!(
                        "{choice:?} more {more}: ring {ring:06x}, not the accent {:06x}",
                        applied.accent
                    ));
                }
                // The field's own 1 px edge takes the accent too, so a
                // quad in the ring colour alone does not show its ring.
                if !rings
                    .iter()
                    .any(|(_, drawn)| (*drawn - width).abs() < px(0.01))
                {
                    failures.push(format!(
                        "{choice:?} more {more}: {id} draws no {width:?} ring, only {rings:?}"
                    ));
                }
                for (layer, _) in rings {
                    for (surface, beneath) in [
                        ("canvas", applied.canvas),
                        ("panel", applied.panel),
                        ("subtle", applied.subtle),
                    ] {
                        let ratio = contrast(composite(layer, beneath), beneath);
                        if ratio < custom::GRAPHIC {
                            failures.push(format!(
                                "{choice:?} more {more}: {id} at {:.2} on the {surface} \
                                 {beneath:06x}: {ratio:.2}:1",
                                layer.a
                            ));
                        }
                    }
                }
            }
        }
    }
    cx.update(|_, cx| set_more_contrast(false, cx));
    failures.dedup();
    assert!(
        failures.is_empty(),
        "kit focus rings below {}:1\n{}",
        custom::GRAPHIC,
        failures.join("\n")
    );
}

#[gpui::test]
fn palette_application_hands_the_shared_button_its_fills_and_label(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for choice in ThemeChoice::ALL {
            choice.apply(None, cx);
            let palette = choice.palette();
            let label: Hsla = rgb(palette.control_label()).into();
            let expected = ButtonCustomVariant::new(cx)
                .foreground(label)
                .hover(gpui_rgba(palette.control_fill(palette.hover)).into())
                .active(gpui_rgba(palette.control_fill(palette.selected)).into());
            assert_eq!(
                control_button_variant(false),
                ButtonVariant::Custom(expected),
                "{choice:?}"
            );
            // A selected helper reads the same label on the `selected`
            // surface, through the kit's secondary where the label is `text`,
            // and disabled rests on the secondary's `subtle`.
            match control_button_variant(true) {
                ButtonVariant::Secondary => {
                    let theme = Theme::global(cx);
                    assert_eq!(
                        theme.colors.button_secondary_foreground, label,
                        "{choice:?}"
                    );
                    assert_eq!(
                        theme.tokens.button_secondary_active.color,
                        rgb(palette.selected).into(),
                        "{choice:?}"
                    );
                    assert_eq!(
                        theme.colors.button_secondary,
                        rgb(palette.subtle).into(),
                        "{choice:?}"
                    );
                }
                ButtonVariant::Custom(variant) => {
                    assert!(
                        matches!(choice, ThemeChoice::KanagawaLotus | ThemeChoice::OneDark),
                        "{choice:?} leaves the kit's secondary"
                    );
                    let expected = ButtonCustomVariant::new(cx)
                        .foreground(label)
                        .active(rgb(palette.selected).into())
                        .disabled(rgb(palette.subtle).into());
                    assert_eq!(variant, expected, "{choice:?}");
                }
                variant => panic!("{choice:?} selects with {variant:?}"),
            }
            // Every focused Button draws 2 px of full `ring`, this `accent`,
            // 1 px outside its edge.
            let theme = Theme::global(cx);
            assert_eq!(
                theme.button_focus_ring,
                FocusRing {
                    width: px(2.),
                    gap: px(1.),
                    opacity: 1.,
                },
                "{choice:?}"
            );
            assert_eq!(theme.colors.ring, rgb(palette.accent).into());
            // A hovered selected helper tints its fill and leaves the
            // whole Button, ring included, at full opacity.
            let hovered = control_selected_hover(StyleRefinement::default());
            assert_eq!(
                hovered.background,
                Some(rgb(palette.row_hover(true)).into()),
                "{choice:?}"
            );
            assert_eq!(hovered.opacity, None, "{choice:?}");
        }
    });
}

#[test]
fn appearance_choices_round_trip_and_unknown_choices_have_safe_defaults() {
    for choice in ThemeChoice::ALL {
        let encoded = serde_json::to_string(&choice).unwrap();
        assert_eq!(
            serde_json::from_str::<ThemeChoice>(&encoded).unwrap(),
            choice
        );
    }
    assert_eq!(
        serde_json::from_str::<ThemeChoice>("\"future-theme\"").unwrap(),
        ThemeChoice::Midnight
    );
    assert_eq!(
        serde_json::from_str::<Density>("\"future-density\"").unwrap(),
        Density::Comfortable
    );
    assert!(Density::Compact.history_row_height() < Density::Comfortable.history_row_height());
    assert!(Density::Compact.file_row_height() < Density::Comfortable.file_row_height());
}

#[test]
fn adapted_family_themes_keep_their_storage_names_labels_and_lightness() {
    assert_eq!(ThemeChoice::ALL.len(), 20);
    for (choice, stored, label, light) in [
        (
            ThemeChoice::SolarizedDark,
            "solarized_dark",
            "Solarized Dark",
            false,
        ),
        (
            ThemeChoice::SolarizedLight,
            "solarized_light",
            "Solarized Light",
            true,
        ),
        (ThemeChoice::OneDark, "one_dark", "One Dark", false),
        (ThemeChoice::OneLight, "one_light", "One Light", true),
        (ThemeChoice::RosePine, "rose_pine", "Rosé Pine", false),
        (
            ThemeChoice::RosePineDawn,
            "rose_pine_dawn",
            "Rosé Pine Dawn",
            true,
        ),
        (ThemeChoice::Dracula, "dracula", "Dracula", false),
        (ThemeChoice::Alucard, "alucard", "Alucard", true),
        (
            ThemeChoice::KanagawaWave,
            "kanagawa_wave",
            "Kanagawa Wave",
            false,
        ),
        (
            ThemeChoice::KanagawaLotus,
            "kanagawa_lotus",
            "Kanagawa Lotus",
            true,
        ),
    ] {
        assert!(ThemeChoice::ALL.contains(&choice));
        assert_eq!(serde_json::to_value(choice).unwrap(), stored);
        assert_eq!(
            serde_json::from_value::<ThemeChoice>(stored.into()).unwrap(),
            choice
        );
        assert_eq!(choice.label(), label);
        assert_eq!(choice.is_light(), light);
        // Two words around a middle dot, like "Deep slate · mint".
        let (surface, accent) = choice.description().split_once(" · ").unwrap();
        assert_eq!(surface.split(' ').count(), 2, "{choice:?}");
        assert_eq!(accent.split(' ').count(), 1, "{choice:?}");
    }
}

#[test]
fn saved_daylight_remains_braden_without_changing_its_storage_or_light_mapping() {
    let saved =
        r#"{"theme":"daylight","follow_system":false,"density":"compact","code_text_size":19}"#;
    let mut settings: crate::preferences::AppSettings = serde_json::from_str(saved).unwrap();
    assert_eq!(
        settings.theme,
        custom::ThemeSelection::BuiltIn(ThemeChoice::Daylight)
    );
    assert_eq!(ThemeChoice::Daylight.label(), "Braden");
    assert_eq!(
        serde_json::to_value(&settings).unwrap()["theme"],
        "daylight"
    );
    assert_eq!(settings.density, Density::Compact);
    assert_eq!(settings.code_text_size, 19);
    let resolved = |settings: &crate::preferences::AppSettings, appearance| {
        settings.resolved_theme(appearance, &[]).selection
    };
    let built_in = custom::ThemeSelection::BuiltIn;
    assert_eq!(resolved(&settings, false), built_in(ThemeChoice::Daylight));
    settings.follow_system = true;
    for light in [
        ThemeChoice::Daylight,
        ThemeChoice::Porcelain,
        ThemeChoice::Sandstone,
        ThemeChoice::SolarizedLight,
        ThemeChoice::OneLight,
        ThemeChoice::RosePineDawn,
        ThemeChoice::Alucard,
        ThemeChoice::KanagawaLotus,
    ] {
        settings.theme = built_in(light);
        assert_eq!(resolved(&settings, true), built_in(ThemeChoice::Daylight));
        assert_eq!(resolved(&settings, false), built_in(ThemeChoice::Midnight));
    }
}

/// The font that an element under [`CodeFont`] hands its descendants, and
/// so the font that the kit's editor and the diff gutter shape with
/// (`window.text_style().font()`), while the code family is `family`.
fn code_text_font(cx: &mut gpui::TestAppContext, family: &'static str) -> gpui_kit::Font {
    use gpui_kit::{Context, Font, IntoElement, ParentElement, Render, canvas, div};
    use std::{cell::RefCell, rc::Rc};

    struct Probe(Rc<RefCell<Option<Font>>>);
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let seen = self.0.clone();
            div().code_font(cx).child(
                canvas(
                    move |_, window, _| *seen.borrow_mut() = Some(window.text_style().font()),
                    |_, _, _, _| {},
                )
                .w(px(1.))
                .h(px(1.)),
            )
        }
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        Theme::global_mut(cx).mono_font_family = family.into();
    });
    let seen = Rc::new(RefCell::new(None));
    let probe = Probe(seen.clone());
    let (_, cx) = cx.add_window_view(move |_, _| probe);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    seen.borrow().clone().expect("the probe painted")
}

/// Code text draws what was typed: a desktop family shapes with its
/// ligatures off.
#[gpui::test]
fn code_text_shapes_the_code_family_without_ligatures(cx: &mut gpui::TestAppContext) {
    let font = code_text_font(cx, "Desktop Mono");
    assert_eq!(font.family, "Desktop Mono");
    assert_eq!(font.features.is_calt_enabled(), Some(false));
    assert!(font.features.tag_value_list().contains(&("liga".into(), 0)));
}

/// The bundled family has no ligatures to turn off, so its code text
/// shapes with no explicit feature, which is faster.
#[gpui::test]
fn code_text_in_the_bundled_family_shapes_without_features(cx: &mut gpui::TestAppContext) {
    let bundled = crate::desktop_text::BUNDLED_CODE_FAMILY;
    let font = code_text_font(cx, bundled);
    assert_eq!(font.family, bundled);
    assert!(font.features.tag_value_list().is_empty(), "{font:?}");
}

/// A checked Switch's knob reads on its accent track in every palette:
/// the thumb takes the accent's label colour (`accent_foreground`, the
/// primary Button's) at 3:1 or more, where `text` drew it at 1.0:1 in
/// Omarchy Kanagawa, whose accent is its text colour. An unchecked thumb
/// stays `text` on the `border` track, and a disabled checked Switch fades
/// its accent label with the track. Built-ins and every embedded Omarchy
/// fixture.
#[gpui::test]
fn every_palette_paints_a_checked_switch_knob_that_reads_on_its_track(
    cx: &mut gpui::TestAppContext,
) {
    use gpui_kit::component::Disableable as _;
    use gpui_kit::component::switch::{self, Switch};
    use gpui_kit::{
        Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
        ScaledPixels, Styled as _, div, point,
    };

    const SWITCHES: [(&str, bool, bool); 3] = [
        ("switch-on", true, false),
        ("switch-off", false, false),
        ("switch-on-disabled", true, true),
    ];
    struct Switches;
    impl Render for Switches {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .gap_4()
                .p_4()
                .children(SWITCHES.map(|(id, checked, disabled)| {
                    div()
                        .debug_selector(move || id.into())
                        .child(Switch::new(id).checked(checked).disabled(disabled))
                }))
        }
    }
    // The fills inside the switch at `id`: its track, then its thumb.
    let painted = |cx: &mut gpui_kit::VisualTestContext, id: &'static str| {
        let switch = cx.debug_bounds(id).expect("the switch");
        cx.update(|window, _| {
            let scale = window.scale_factor();
            let logical = |value: ScaledPixels| px(value.as_f32() / scale);
            let fills = window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .filter(|quad| {
                    let drawn = Bounds::from_corners(
                        point(logical(quad.bounds.left()), logical(quad.bounds.top())),
                        point(logical(quad.bounds.right()), logical(quad.bounds.bottom())),
                    );
                    switch.dilate(px(1.)).contains(&drawn.origin)
                        && drawn.right() <= switch.right() + px(1.)
                })
                .map(|quad| quad.background)
                .collect::<Vec<_>>();
            let [track, thumb] = fills[..] else {
                panic!("{id} paints a track and a thumb: {fills:?}");
            };
            (track, thumb)
        })
    };
    let fill = |color: u32, faded: bool| {
        let fill = Background::from(Hsla::from(rgb(color)));
        if faded { fill.opacity(0.5) } else { fill }
    };

    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|_, _| Switches);
    let mut failures = Vec::new();
    for (name, palette) in test_palettes() {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            cx.update(|_, cx| switch::thumb_color(true, cx)),
            Hsla::from(rgb(palette.accent_foreground)),
            "{name}"
        );
        for (id, checked, disabled) in SWITCHES {
            let track = if checked {
                palette.accent
            } else {
                palette.border
            };
            let thumb = if checked {
                palette.accent_foreground
            } else {
                palette.text
            };
            assert_eq!(
                painted(cx, id),
                (fill(track, disabled), fill(thumb, disabled)),
                "{name}: {id}"
            );
        }
        let knob = contrast(palette.accent_foreground, palette.accent);
        if knob < custom::GRAPHIC {
            failures.push(format!("{name} {knob:.2}:1"));
        }
    }
    assert!(
        failures.is_empty(),
        "checked knob below {}:1 on its track: {}",
        custom::GRAPHIC,
        failures.join(", ")
    );
}

/// Every pair of a small label and a surface it sits on that falls below
/// the text rule with its rasterization margin, as `name label on surface
/// ratio`.
fn small_label_failures(name: &str, what: &str, label: u32, surfaces: &[u32]) -> Option<String> {
    let low = surfaces
        .iter()
        .map(|&surface| (surface, contrast(label, surface)))
        .filter(|&(_, ratio)| ratio < LABEL_RULE + LABEL_MARGIN)
        .map(|(surface, ratio)| format!("{surface:06x} {ratio:.2}:1"))
        .collect::<Vec<_>>();
    (!low.is_empty()).then(|| format!("{name} {what} {label:06x} on {}", low.join(", ")))
}

/// History's reference badge draws its label in its graph lane's color on a
/// tint of that color, and the lanes are fitted only as graphics, to 3:1:
/// on the selected row Omarchy Catppuccin Latte's first lane read 2.95:1.
/// Every palette's badge labels clear the text rule with the margin on
/// their tint over each History row surface, and a lane that already does
/// keeps its own color. The applied palette installs them.
#[gpui::test]
fn every_palette_fits_history_reference_labels_on_their_tint(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let mut failures = Vec::new();
    let mut lanes_below = 0;
    for (name, palette) in test_palettes() {
        let labels = cx.update(|cx| {
            palette.apply(palette.is_light(), None, cx);
            super::labels(cx)
        });
        assert_eq!(labels, Labels::fit(palette), "{name} installs its labels");
        let rows = [
            palette.canvas,
            palette.hover,
            palette.selected,
            palette.row_hover(true),
        ];
        for (lane, (color, label)) in crate::graph::palette_colors(palette)
            .into_iter()
            .zip(labels.refs)
            .enumerate()
        {
            let tint = Rgba {
                a: REF_TINT_ALPHA as f32 / 255.,
                ..rgb(color)
            };
            let badges = rows.map(|row| composite(tint, row));
            let own = small_label_failures(name.as_str(), "", color, &badges);
            lanes_below += usize::from(own.is_some());
            if own.is_none() {
                assert_eq!(label, color, "{name} lane {lane} keeps its color");
            }
            failures.extend(small_label_failures(
                &name,
                &format!("lane {lane}"),
                label,
                &badges,
            ));
        }
    }
    assert!(lanes_below > 0, "some lane needs its label fitted");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A selected sidebar row (repository navigation and the project list)
/// labels itself in `text` at the text rule on the selected and hovered
/// selected rows, where `accent`, fitted only to 3:1, fell to 3.0:1 in
/// Omarchy Solitude; its icon keeps `accent` at 3:1 as a graphic. Unselected
/// rows read on the panel and its hover.
#[test]
fn every_palette_labels_a_selected_sidebar_row_in_readable_text() {
    let mut failures = Vec::new();
    let mut accent_below = false;
    for (name, palette) in test_palettes() {
        for (active, rows) in [
            (true, [palette.selected, palette.row_hover(true)]),
            (false, [palette.panel, palette.hover]),
        ] {
            let (label, icon) = palette.sidebar_row(active);
            for row in rows {
                let (text, graphic) = (contrast(label, row), contrast(icon, row));
                if text < LABEL_RULE || graphic < custom::GRAPHIC {
                    failures.push(format!(
                        "{name} active {active} on {row:06x}: label {text:.2}:1, icon {graphic:.2}:1"
                    ));
                }
                accent_below |= active && contrast(palette.accent, row) < LABEL_RULE;
            }
        }
    }
    assert!(
        accent_below,
        "accent alone would miss the text rule somewhere"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The Unstaged badge (`modified` on `subtle`) and the Staged and "N
/// staged" badges (`added` on its diff tile) are 10 px labels whose tokens
/// keep only the text rule, and `modified` not even that: 4.1:1 in Omarchy
/// Rose Pine and 3.9:1 in Kanagawa Lotus. Every palette's badge labels
/// clear the rule with the margin, and a token that already does is kept.
#[test]
fn every_palette_fits_staged_and_unstaged_badge_labels() {
    let mut failures = Vec::new();
    let mut moved = 0;
    for (name, palette) in test_palettes() {
        let labels = Labels::fit(palette);
        for (what, token, label, surface) in [
            (
                "unstaged",
                palette.modified,
                labels.unstaged,
                palette.subtle,
            ),
            (
                "staged",
                palette.added,
                labels.added,
                palette.added_background,
            ),
        ] {
            match small_label_failures(&name, what, token, &[surface]) {
                Some(_) => moved += 1,
                None => assert_eq!(label, token, "{name} keeps its {what} color"),
            }
            failures.extend(small_label_failures(&name, what, label, &[surface]));
        }
    }
    assert!(moved > 0, "some palette needs a badge label fitted");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The surfaces a disabled control's label rests on, by name: the page's
/// surfaces and the raised menu surface.
fn disabled_label_surfaces(palette: Palette) -> [(&'static str, u32); 4] {
    [
        ("canvas", palette.canvas),
        ("panel", palette.panel),
        ("subtle", palette.subtle),
        ("menu", palette.raised()),
    ]
}

/// The disabled Buttons [`DisabledControls`] draws, by kind: the kit's
/// secondary (Fetch and Pull) and primary (Push, Commit), the shared helper
/// at rest and selected (a dialog's default action), the kit's ghost, and
/// its default Button with no variant (Settings' Save). A dialog's Cancel
/// is the bordered secondary (`tags::dialog_cancel`).
const DISABLED_BUTTONS: [&str; 6] = [
    "secondary",
    "primary",
    "control",
    "control-selected",
    "ghost",
    "default",
];

/// `kind-surface`, the debug selector [`DisabledControls`] gives a Button.
fn disabled_selector(kind: &str, surface: &str) -> &'static str {
    Box::leak(format!("{kind}-{surface}").into_boxed_str())
}

/// Each kind of disabled Button and a disabled Settings switch row, in a
/// row on each of [`disabled_label_surfaces`] of the applied palette.
struct DisabledControls;

impl gpui::Render for DisabledControls {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui_kit::component::button::{Button, ButtonVariants as _};
        use gpui_kit::component::{Disableable as _, Selectable as _};
        use gpui_kit::{InteractiveElement as _, ParentElement as _, div};

        let surfaces = disabled_label_surfaces(palette(cx));
        div()
            .flex()
            .flex_col()
            .children(surfaces.map(|(surface, color)| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .bg(rgb(color))
                    .children(DISABLED_BUTTONS.map(|kind| {
                        let button = Button::new(disabled_selector(kind, surface))
                            .debug_selector(move || format!("{kind}-{surface}"))
                            .label("Push");
                        match kind {
                            "secondary" => button.secondary(),
                            "primary" => button.primary(),
                            "control" => button.with_variant(control_button_variant(false)),
                            "control-selected" => button
                                .with_variant(control_button_variant(true))
                                .selected(true),
                            "ghost" => button.ghost(),
                            _ => button,
                        }
                        .disabled(true)
                    }))
                    // Follow system appearance's row as Settings draws
                    // it, its title the switch's label.
                    .child(
                        crate::settings::switch_row(
                            disabled_selector("switch", surface),
                            crate::settings::FOLLOW_SYSTEM,
                            crate::settings::FOLLOW_SYSTEM,
                            "Omarchy follows your desktop theme",
                            &[],
                            cx,
                        )
                        .disabled(true),
                    )
            }))
    }
}

/// Disabled Buttons, Switch labels, menu items with their shortcuts and
/// command palette rows share one dim, `Palette::disabled_label`, which the
/// kit's `disabled_foreground` returns. There were two, and in light
/// palettes both fell near 2:1: Buttons and menus drew `muted` at half
/// strength (Omarchy Catppuccin Latte's disabled Push 2.06:1, Flexoki
/// Light's toolbar 2.12:1), and Switch labels `text` at half (the Follow
/// system label 2.35:1 in Latte). In every palette the dim reads 3:1 on
/// every surface a disabled label rests on, the page's and the menu's, and
/// on the fill each disabled Button paints there, the kit's default
/// Button's translucent one included: that one was left out of the fit, so
/// Midnight's disabled Save on Settings' card read 3.002:1, under 3
/// natively, and Nord's and Solarized Dark's below 3:1. It keeps the margin
/// 1x rendering takes above that, unless the first step that does would
/// break the 60% bound below (Kanagawa Lotus's highlighted row), and
/// Midnight keeps it. Where it had to move off half strength it reads no
/// more than 0.15 above its floor on its closest surface, about 3:1. It keeps
/// at most 60% of enabled secondary text's contrast on each of those and
/// on the selected row (a highlighted disabled command), so disabled stays
/// clearly fainter than enabled text.
#[gpui::test]
fn every_palette_shares_one_disabled_dim_that_reads_three_to_one(cx: &mut gpui::TestAppContext) {
    use gpui_kit::component::switch;

    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|_, _| DisabledControls);
    let mut failures = Vec::new();
    let (mut moved, mut kept) = (0, 0);
    for (name, palette) in test_palettes() {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let (dim, switch_label) = cx.update(|_, cx| {
            (
                Theme::global(cx).disabled_foreground(),
                switch::disabled_label_color(cx),
            )
        });
        assert_eq!(
            dim,
            Hsla::from(gpui_rgba(palette.disabled_label())),
            "{name} dim"
        );
        assert_eq!(switch_label, dim, "{name} Switch label");
        // A disabled Settings switch's title, its label, draws in it.
        assert_eq!(
            drawn_color(&format!(
                "setting-title: {}",
                crate::settings::FOLLOW_SYSTEM
            )),
            Some(dim),
            "{name} disabled switch's title"
        );
        let dim = dim.to_rgb();
        // The dim's contrast on `beneath`, recording a failure where it
        // keeps more than 60% of enabled secondary text's.
        let check = |failures: &mut Vec<String>, what: &str, beneath: u32| {
            let disabled = contrast(composite(dim, beneath), beneath);
            let enabled = contrast(palette.muted, beneath);
            if disabled > enabled * DISABLED_SHARE {
                failures.push(format!(
                    "{name} {what} {beneath:06x}: disabled {disabled:.2}:1, secondary text {enabled:.2}:1"
                ));
            }
            disabled
        };
        // A Switch's label and a menu item's lie on the surface itself,
        // a disabled Button's on the fills it paints there.
        let mut readings = Vec::new();
        for (surface, color) in disabled_label_surfaces(palette) {
            readings.push((format!("label on the {surface}"), color));
            for kind in DISABLED_BUTTONS {
                let bounds = cx
                    .debug_bounds(disabled_selector(kind, surface))
                    .expect("the button");
                let (fills, _) = painted_button(cx, bounds);
                let painted = fills.iter().fold(color, |beneath, fill| {
                    composite(fill.as_solid().expect("a solid fill").to_rgb(), beneath)
                });
                readings.push((format!("{kind} on the {surface}"), painted));
            }
        }
        let mut lowest = f64::MAX;
        for (what, beneath) in &readings {
            let disabled = check(&mut failures, what, *beneath);
            lowest = lowest.min(disabled);
            if disabled < DISABLED_LABEL {
                failures.push(format!("{name} {what} {beneath:06x}: {disabled:.3}:1"));
            }
        }
        let on_selected = check(&mut failures, "highlighted row", palette.selected);
        if on_selected < 1.5 {
            failures.push(format!("{name} highlighted row {on_selected:.2}:1"));
        }
        // Below the margin only where the first stronger step that keeps
        // it would take more than 60% of secondary text's contrast.
        let floor = if lowest >= DISABLED_LABEL + DISABLED_LABEL_MARGIN {
            kept += 1;
            DISABLED_LABEL + DISABLED_LABEL_MARGIN
        } else {
            let stronger = (1..=16u8)
                .map(|step| Rgba {
                    a: dim.a + f32::from(step) / 64.,
                    ..dim
                })
                .take_while(|layer| layer.a <= 0.75)
                .find(|&layer| {
                    readings.iter().all(|&(_, beneath)| {
                        contrast(composite(layer, beneath), beneath)
                            >= DISABLED_LABEL + DISABLED_LABEL_MARGIN
                    })
                });
            let fainter = |layer: Rgba| {
                readings
                    .iter()
                    .map(|&(_, beneath)| beneath)
                    .chain([palette.selected])
                    .all(|beneath| {
                        contrast(composite(layer, beneath), beneath)
                            <= contrast(palette.muted, beneath) * DISABLED_SHARE
                    })
            };
            if stronger.is_some_and(fainter) {
                failures.push(format!(
                    "{name} reads {lowest:.3}:1 at its closest, where {stronger:?} keeps the margin"
                ));
            }
            DISABLED_LABEL
        };
        if name == "Midnight" && floor == DISABLED_LABEL {
            failures.push(format!("Midnight reads {lowest:.3}:1 at its closest"));
        }
        if palette.disabled_label().a > 0.5 {
            moved += 1;
            if lowest > floor + 0.15 {
                failures.push(format!("{name} reads {lowest:.2}:1 at its closest"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(moved > 0, "some palette needs more than half strength");
    assert!(kept > 0, "some palette keeps the margin");
}
/// A disabled primary Button (Push, Commit) rests on the neutral fill a
/// disabled secondary one does, the opaque `subtle` beside it, as does the
/// shared helper's selected look (the command palette's Open command). The
/// primary drew a 15% tint of the accent instead, grey #2a292f in Omarchy
/// Retro-82's navy toolbar beside a navy Fetch, and the helper, where a
/// palette moves its label (Omarchy Catppuccin Latte), drew no fill at all.
/// Every palette, on each page surface and the menu's.
#[gpui::test]
fn every_palette_rests_disabled_primary_buttons_on_the_neutral_fill(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|_, _| DisabledControls);
    let mut failures = Vec::new();
    for (name, palette) in test_palettes() {
        cx.update(|window, cx| palette.apply(palette.is_light(), Some(window), cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let neutral = vec![Background::from(Hsla::from(rgb(palette.subtle)))];
        for (surface, _) in disabled_label_surfaces(palette) {
            for kind in ["secondary", "primary", "control-selected"] {
                let bounds = cx
                    .debug_bounds(disabled_selector(kind, surface))
                    .expect("the button");
                let (fills, _) = painted_button(cx, bounds);
                if fills != neutral {
                    failures.push(format!("{name} {kind} on the {surface}: {fills:?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A disabled Button that keeps a fill, the kit's secondary (Fetch, a
/// disabled Cancel) and primary (Push, a dialog's default action) and
/// the shared helper's selected look (Open command), keeps its shape with
/// the kit's disabled edge on each page surface and the menu's, a
/// dialog's canvas among them, in every palette: its neutral `subtle`
/// fill stood 1.03 to 1.05:1 off the dialog in Rosé Pine, Graphite,
/// Midnight, Braden and Ember, and no more than 1.34:1 off any surface,
/// so it drew only its dim label. The edge holds 1.5:1 against each
/// surface and the fill, and stays fainter than the dialog Cancel's
/// edge; the Buttons that turn transparent, the shared helper at rest and
/// the ghost, draw none. With More contrast on, each disabled Button
/// draws the same edge, the palette's own 1.5:1 fit, which reads below
/// the 3:1 resting edge every enabled control on a fill gains, on every
/// surface: taken from the adjusted `border`, it was the resting edge
/// itself (Midnight's #606976 on Delete branch… and Rename… beside it).
#[gpui::test]
fn every_disabled_button_that_keeps_a_fill_keeps_its_shape_with_an_edge(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|_, _| DisabledControls);
    let mut failures = Vec::new();
    for (palette_name, palette) in test_palettes() {
        for more in [false, true] {
            let name = if more {
                format!("{palette_name} with More contrast")
            } else {
                palette_name.clone()
            };
            cx.update(|window, cx| {
                set_more_contrast(more, cx);
                palette.apply(palette.is_light(), Some(window), cx);
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let applied = if more {
                palette.with_more_contrast()
            } else {
                palette
            };
            let edge = palette.disabled_edge();
            for (surface, color) in disabled_label_surfaces(palette) {
                for kind in DISABLED_BUTTONS {
                    let expected = match kind {
                        "secondary" | "primary" | "control-selected" => {
                            vec![Hsla::from(rgb(edge))]
                        }
                        // The kit's default Button keeps a border of its own.
                        "default" => continue,
                        _ => Vec::new(),
                    };
                    let edges =
                        crate::views::tests::painted_edges(cx, disabled_selector(kind, surface));
                    if edges != expected {
                        failures.push(format!("{name} {kind} on the {surface}: {edges:?}"));
                    }
                }
                let ratio = contrast(edge, color);
                if ratio < DISABLED_EDGE {
                    failures.push(format!("{name}: #{edge:06x} {ratio:.3}:1 on the {surface}"));
                }
                let resting = applied.resting_edge();
                let enabled = contrast(resting, color);
                if more && ratio >= enabled {
                    failures.push(format!(
                        "{name}: #{edge:06x} {ratio:.3}:1 on the {surface}, no fainter than \
                         the resting edge #{resting:06x} at {enabled:.3}:1"
                    ));
                }
            }
            let on_fill = contrast(edge, palette.subtle);
            if on_fill < DISABLED_EDGE {
                failures.push(format!("{name}: #{edge:06x} {on_fill:.3}:1 on its fill"));
            }
            let cancel = contrast(applied.dialog_edge(), palette.canvas);
            if contrast(edge, palette.canvas) >= cancel {
                failures.push(format!("{name}: #{edge:06x} no fainter than Cancel's edge"));
            }
        }
    }
    cx.update(|_, cx| set_more_contrast(false, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Compare's gutter numbers on added and removed lines are code-size text
/// on the diff tiles, in `added` and `removed`, which the palettes fit to
/// the text rule with no room: 4.51:1 and 4.57:1 in Omarchy Flexoki Light,
/// read natively at 4.32:1 and 4.43:1. Every palette's numbers clear the
/// rule with the margin on their tile, keeping a token that already does.
#[test]
fn every_palette_fits_gutter_numbers_on_the_diff_tiles() {
    let mut failures = Vec::new();
    let mut moved = 0;
    for (name, palette) in test_palettes() {
        let labels = Labels::fit(palette);
        for (what, token, label, tile) in [
            (
                "added",
                palette.added,
                labels.added,
                palette.added_background,
            ),
            (
                "removed",
                palette.removed,
                labels.removed,
                palette.removed_background,
            ),
        ] {
            match small_label_failures(&name, what, token, &[tile]) {
                Some(_) => moved += 1,
                None => assert_eq!(label, token, "{name} keeps its {what} color"),
            }
            failures.extend(small_label_failures(&name, what, label, &[tile]));
        }
    }
    assert!(moved > 0, "some palette needs its gutter numbers fitted");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Count chips (the Changes count, the changed-file count and "0 staged")
/// draw their number in `text` on their `hover` fill, fitted with the
/// margin: they drew `muted`, which secondary text keeps at the text rule
/// alone, and 11 px digits read 4.32:1 natively in Omarchy Catppuccin
/// Latte. Every palette's count clears the rule with the margin.
#[test]
fn every_palette_fits_count_chip_numbers() {
    let mut failures = Vec::new();
    for (name, palette) in test_palettes() {
        let count = Labels::fit(palette).count;
        if small_label_failures(&name, "", palette.text, &[palette.hover]).is_none() {
            assert_eq!(count, palette.text, "{name} keeps its text color");
        }
        failures.extend(small_label_failures(
            &name,
            "count",
            count,
            &[palette.hover],
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Menus, popovers, selects and tooltips float on `Palette::raised`, the
/// lightest page surface, edged by `Palette::raised_edge`: the Linux Menu
/// used to share the panel's color, 1.00:1 against the header it drops
/// from, with a 10% text-colored hairline at 1.12:1 to 1.15:1 in Omarchy
/// Catppuccin Latte, Flexoki Light, Ethereal and Hackerman, and `border`
/// alone would read 1.17:1 on Omarchy Matte Black's raised surface. In
/// every palette the surface is at least as light as each page surface,
/// its edge reads 1.4:1 against the surface and against every page surface
/// beneath it, the canvas included, separators take that edge, text and
/// secondary text clear the text rule on it, and the menu's hover row keeps
/// the hover step off it. An edge fitted to the border rule's 1.3 alone
/// (1.31:1 in Braden, 1.33:1 in Omarchy Everforest) drew 1.21:1 to 1.26:1
/// once antialiased; one that has to move off `border` stops within 0.03
/// above 1.4 on its closest surface, rather than a coarse step past it.
#[gpui::test]
fn every_palette_raises_menus_above_the_page_with_a_border_edge(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let mut failures = Vec::new();
    let mut moved = 0;
    for (name, palette) in test_palettes() {
        let (popover, ring, separator, hover) = cx.update(|cx| {
            palette.apply(palette.is_light(), None, cx);
            let theme = Theme::global(cx);
            (
                theme.popover,
                gpui_kit::component::popover_ring(cx),
                gpui_kit::component::menu_separator(cx),
                theme.tokens.accent.color,
            )
        });
        assert_eq!(popover, Hsla::from(rgb(palette.raised())), "{name}");
        assert_eq!(ring, Hsla::from(rgb(palette.raised_edge())), "{name} edge");
        // Separators lie on the menu, where `border` alone read 1.16:1 in
        // Omarchy Matte Black and 1.25:1 in Lumon.
        assert_eq!(separator, ring, "{name} separator");
        // The menu's hover row; the keyboard's cursor takes `list_active`.
        assert_eq!(hover, Hsla::from(rgb(palette.hover)), "{name} hover row");
        let (popover, hover) = (palette.raised(), palette.hover);
        let mut check = |what: &str, ratio: f64, minimum: f64| {
            if ratio < minimum {
                failures.push(format!("{name} {what} {ratio:.3}:1, needs {minimum}"));
            }
        };
        for (page, surface) in [
            ("subtle", palette.subtle),
            ("canvas", palette.canvas),
            ("panel", palette.panel),
        ] {
            // Never darker than a page surface: a ratio of 1 where it is not.
            let lift = if luminance(popover) < luminance(surface) {
                0.
            } else {
                1.
            };
            check(&format!("lift over {page}"), lift, 1.);
            let edge = palette.raised_edge();
            check(&format!("edge on {page}"), contrast(edge, surface), 1.4);
        }
        check(
            "edge on the menu",
            contrast(palette.raised_edge(), popover),
            1.4,
        );
        check(
            "text on the menu",
            contrast(palette.text, popover),
            LABEL_RULE,
        );
        check(
            "muted on the menu",
            contrast(palette.muted, popover),
            LABEL_RULE,
        );
        check("hover row on the menu", contrast(hover, popover), 1.08);
        let edge = palette.raised_edge();
        if edge != palette.border {
            moved += 1;
            let closest = [popover, palette.subtle, palette.canvas, palette.panel]
                .map(|surface| contrast(edge, surface))
                .into_iter()
                .fold(f64::MAX, f64::min);
            if closest > 1.43 {
                failures.push(format!(
                    "{name} edge {edge:06x} {closest:.3}:1 at its closest"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(moved > 0, "some palette's edge moves off its border");
}

/// A custom theme on Midnight whose secondary text, gutter numbers and
/// dividers sit close to its surfaces: 2.33:1, 1.81:1 and 1.15:1 off its
/// canvas.
fn faint_custom_palette() -> Palette {
    let mut custom = CustomTheme::from_base(1, "Faint", ThemeChoice::Midnight);
    custom.palette.set(TokenKind::Muted, 0x4a5262);
    custom.palette.set(TokenKind::LineNumber, 0x3a4252);
    custom.palette.set(TokenKind::Border, 0x1b2230);
    custom.palette
}

/// Every palette the More contrast tests judge: `test_palettes` and
/// [`faint_custom_palette`].
fn more_contrast_palettes() -> Vec<(String, Palette)> {
    let mut palettes = test_palettes();
    palettes.push(("custom Faint".into(), faint_custom_palette()));
    palettes
}

/// The surfaces More contrast reads secondary text and gutter numbers
/// on, by name, each with whether it is momentary: the six surfaces of
/// the readability rules, the raised surface and the added and removed
/// line tints, of which hover and the hovered selected row are momentary.
fn more_contrast_text_surfaces(palette: Palette) -> [(&'static str, u32, bool); 9] {
    [
        ("canvas", palette.canvas, false),
        ("panel", palette.panel, false),
        ("subtle", palette.subtle, false),
        ("raised", palette.raised(), false),
        ("selected", palette.selected, false),
        ("added lines", palette.added_background, false),
        ("removed lines", palette.removed_background, false),
        ("hover", palette.hover, true),
        ("hovered selected row", palette.row_hover(true), true),
    ]
}

/// What More contrast holds secondary text and gutter numbers to on
/// `surface`: 7:1 where it rests and the text rule's 4.5:1 where it is
/// `momentary`, or as far as `text` itself reads there where that is
/// lower.
fn more_contrast_text_floor(palette: Palette, surface: u32, momentary: bool) -> f64 {
    let floor = if momentary {
        LABEL_RULE
    } else {
        MORE_CONTRAST_TEXT
    };
    floor.min(contrast(palette.text, surface))
}

/// The surfaces dividers and the edges fitted from them rest on, by name.
fn more_contrast_edge_surfaces(palette: Palette) -> [(&'static str, u32); 4] {
    [
        ("canvas", palette.canvas),
        ("panel", palette.panel),
        ("subtle", palette.subtle),
        ("raised", palette.raised()),
    ]
}

/// More contrast on every palette, Omarchy's on Linux and a custom one
/// with faint tokens among them: secondary text and gutter numbers move
/// toward `text` by the least step of `mix` that reads 7:1 on the
/// resting surfaces and both line tints and 4.5:1 on hover and the
/// hovered selected row, or as `text` reads where that is lower, and
/// reads above `text` on none of them, so they stay apart from `text`
/// wherever `text` reads 7:1 on every resting surface, a hover falling
/// short included, and never pass it; dividers hold 3:1 on the
/// canvas, panel, subtle and raised surfaces, and so do the popup and
/// resting edges fitted from them. Secondary text and gutter numbers are
/// measured on the line tints as adjusted, where a line moved from a
/// changed word it is drawn under
/// (`more_contrast_moves_a_line_its_word_cannot_stand_apart_from`). No
/// other token moves, a token already past its floor keeps its value, so
/// raising the floors of an adjusted palette changes nothing, and where
/// no line tint moves, adjusting twice is adjusting once.
#[test]
fn more_contrast_raises_secondary_text_gutters_and_dividers_alone() {
    let mut failures = Vec::new();
    let mut moved = std::collections::BTreeMap::<&str, usize>::new();
    // Palettes whose `text` misses 7:1 only on a momentary surface.
    let mut hover_short = Vec::new();
    for (name, palette) in more_contrast_palettes() {
        let adjusted = palette.with_more_contrast();
        let surfaces = more_contrast_text_surfaces(adjusted);
        // A step reads where it reaches its floor and reads above `text`
        // on no surface.
        let reads = |color: u32| {
            surfaces.iter().all(|&(_, surface, momentary)| {
                let ratio = contrast(color, surface);
                ratio >= more_contrast_text_floor(palette, surface, momentary)
                    && ratio <= contrast(palette.text, surface)
            })
        };
        let text_short = |momentary: bool| {
            surfaces.iter().any(|&(_, surface, on_hover)| {
                on_hover == momentary && contrast(palette.text, surface) < MORE_CONTRAST_TEXT
            })
        };
        if text_short(true) && !text_short(false) {
            hover_short.push(name.clone());
        }
        for (token, before, after) in [
            ("muted", palette.muted, adjusted.muted),
            ("line_number", palette.line_number, adjusted.line_number),
        ] {
            // Where `text` reads 7:1 on every resting surface, the token
            // reads below it on one of them: it stays secondary.
            let apart = surfaces.iter().any(|&(_, surface, momentary)| {
                !momentary && contrast(after, surface) < contrast(palette.text, surface)
            });
            if !text_short(false) && !apart {
                failures.push(format!(
                    "{name} {token} #{after:06x} reads as text #{:06x} does",
                    palette.text
                ));
            }
            for (surface, color, momentary) in surfaces {
                let (ratio, floor) = (
                    contrast(after, color),
                    more_contrast_text_floor(palette, color, momentary),
                );
                if ratio < floor {
                    failures.push(format!(
                        "{name} {token} #{after:06x} on {surface} {ratio:.3}:1, needs {floor:.3}"
                    ));
                }
            }
            // The least step toward `text` that reads.
            match (0..=256).find(|&step| mix(before, palette.text, step) == after) {
                Some(step) if step == 0 || !reads(mix(before, palette.text, step - 1)) => {}
                step => failures.push(format!(
                    "{name} {token} #{before:06x} -> #{after:06x} is not the least reading \
                     step toward text: {step:?}"
                )),
            }
            if after != before {
                *moved.entry(token).or_default() += 1;
            }
        }
        for (surface, color) in more_contrast_edge_surfaces(palette) {
            for (edge, value) in [
                ("border", adjusted.border),
                ("popup edge", adjusted.raised_edge()),
                ("resting edge", adjusted.resting_edge()),
            ] {
                let ratio = contrast(value, color);
                if ratio < MORE_CONTRAST_EDGE {
                    failures.push(format!(
                        "{name} {edge} #{value:06x} on {surface} {ratio:.3}:1"
                    ));
                }
            }
        }
        let edge_holds = more_contrast_edge_surfaces(palette)
            .iter()
            .all(|&(_, surface)| contrast(palette.border, surface) >= MORE_CONTRAST_EDGE);
        if edge_holds && adjusted.border != palette.border {
            failures.push(format!("{name} border past its floor moved"));
        }
        if adjusted.border != palette.border {
            *moved.entry("border").or_default() += 1;
        }
        let untouched = Palette {
            muted: palette.muted,
            line_number: palette.line_number,
            border: palette.border,
            added_background: palette.added_background,
            removed_background: palette.removed_background,
            ..adjusted
        };
        if untouched != palette {
            failures.push(format!("{name} moved another token: {untouched:x?}"));
        }
        if adjusted.with_raised_floors() != adjusted {
            failures.push(format!(
                "{name} raises its floors again on the adjusted palette"
            ));
        }
        let lines = |palette: Palette| (palette.added_background, palette.removed_background);
        if lines(adjusted) == lines(palette) && adjusted.with_more_contrast() != adjusted {
            failures.push(format!("{name} moves again when adjusted twice"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        hover_short.iter().any(|name| name == "Tokyo Night"),
        "a palette whose text misses 7:1 only on a hover keeps its secondary text apart: \
         {hover_short:?}"
    );
    // Each adjustment does something somewhere, the faint palette's all three.
    let faint = faint_custom_palette();
    let adjusted = faint.with_more_contrast();
    assert_ne!(adjusted.muted, faint.muted);
    assert_ne!(adjusted.line_number, faint.line_number);
    assert_ne!(adjusted.border, faint.border);
    assert!(moved.values().all(|&count| count > 1), "{moved:?}");
    // Already past every floor, a palette is its own adjustment.
    let strong = Palette {
        muted: faint.text,
        line_number: faint.text,
        border: adjusted.border,
        ..faint
    };
    assert_eq!(strong.with_more_contrast(), strong);
}

/// With More contrast on, what `Palette::apply` installs meets the
/// raised floors in every palette. The `Palette` global is the adjusted
/// palette, and `as_applied` returns it; the kit draws secondary text and
/// gutter numbers from it. Every syntax color and the editor foreground
/// read 7:1, or as `text` reads where that is lower, on the editor
/// background, the active line and both line tints, or stand at `text`'s
/// lightness, where a role's fit stops short of that. The kit's disabled
/// label reads 4.5:1 on every surface a disabled label rests on and on
/// the default Button's disabled fill over each, fainter than the
/// adjusted secondary text there and on the selected row, and at most 75%
/// of its contrast wherever a layer reading 4.5:1 can be. The dividers,
/// the fields' outlines and the popup, dialog and bordered secondary
/// edges hold 3:1 on the surfaces each is fitted for, as the resting edge
/// does, while the disabled edge and the unchecked Switch track's fill
/// and hover keep the chosen palette's values.
#[gpui::test]
fn more_contrast_applies_its_raised_floors_in_every_palette(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let mut failures = Vec::new();
    for (name, chosen) in more_contrast_palettes() {
        let (applied, as_applied, theme) = cx.update(|cx| {
            set_more_contrast(true, cx);
            chosen.apply(chosen.is_light(), None, cx);
            (
                palette(cx),
                chosen.as_applied(cx),
                Theme::global(cx).clone(),
            )
        });
        assert_eq!(applied, chosen.with_more_contrast(), "{name} applied");
        assert_eq!(as_applied, applied, "{name} as applied");
        let hsla = |color: u32| Hsla::from(rgb(color));
        assert_eq!(theme.colors.muted_foreground, hsla(applied.muted), "{name}");
        assert_eq!(theme.colors.border, hsla(applied.border), "{name}");
        assert_eq!(theme.colors.input, hsla(applied.border), "{name}");
        let style = &theme.highlight_theme.style;
        assert_eq!(
            style.editor_line_number,
            Some(hsla(applied.line_number)),
            "{name}"
        );
        let line = |color: Option<Hsla>| packed(color.expect("a configured line"));
        let lines = [
            ("editor background", line(style.editor_background)),
            ("active line", line(style.editor_active_line)),
            ("added lines", applied.added_background),
            ("removed lines", applied.removed_background),
        ];
        for (field, color) in editor_text_colors(&theme) {
            let Some(color) = color else { continue };
            for (surface, background) in lines {
                let floor = MORE_CONTRAST_TEXT.min(contrast(applied.text, background));
                let ratio = contrast(color, background);
                // A role short of its floor at `text`'s lightness stopped there
                // (`more_contrast_raises_syntax_in_lightness_keeping_its_hue`).
                if ratio < floor && !at_text_lightness(color, applied.text) {
                    failures.push(format!(
                        "{name} {field} #{color:06x} on {surface} {ratio:.3}:1, needs {floor:.3}"
                    ));
                }
            }
        }
        let dim = theme.disabled_foreground().to_rgb();
        let page = disabled_label_surfaces(applied);
        // The default Button's disabled fill is a fill, the chosen
        // palette's, as the kit derives it from `input_fill_base`.
        let fill = chosen.default_disabled_fill();
        let beneath = page
            .iter()
            .map(|&(surface, color)| (surface.to_owned(), color))
            .chain(page.iter().map(|&(surface, color)| {
                (
                    format!("disabled fill on {surface}"),
                    composite_color(fill, color),
                )
            }));
        for (surface, color) in beneath {
            let (disabled, secondary) = (
                contrast(composite(dim, color), color),
                contrast(applied.muted, color),
            );
            if disabled < MORE_CONTRAST_DISABLED || disabled >= secondary {
                failures.push(format!(
                    "{name} disabled label on {surface} {disabled:.3}:1, secondary text {secondary:.3}:1"
                ));
            }
        }
        let on_selected = contrast(composite(dim, applied.selected), applied.selected);
        if on_selected >= contrast(applied.muted, applied.selected) {
            failures.push(format!(
                "{name} disabled label on the selected row {on_selected:.3}:1"
            ));
        }
        // The 75% ceiling: wherever a layer of `muted` reads 4.5:1 with
        // the margin and keeps at most 75% of secondary text's contrast,
        // the label keeps it; where none does, no fainter layer reads
        // 4.5:1, so the label passes the ceiling no further than the
        // floor takes it.
        let rests: Vec<u32> = page
            .iter()
            .map(|&(_, color)| color)
            .chain(page.iter().map(|&(_, color)| composite_color(fill, color)))
            .collect();
        let layer = |a: f32| Rgba {
            a,
            ..rgb(applied.muted)
        };
        let reading = |label: Rgba, surface: u32| contrast(composite(label, surface), surface);
        let reads = |label: Rgba, floor: f64| {
            rests
                .iter()
                .all(|&surface| reading(label, surface) >= floor)
        };
        let under_ceiling = |label: Rgba| {
            rests.iter().chain([&applied.selected]).all(|&surface| {
                reading(label, surface)
                    <= contrast(applied.muted, surface) * MORE_CONTRAST_DISABLED_SHARE + 1e-9
            })
        };
        let layers = (0..=28u8).map(|step| layer(0.5 + f32::from(step) / 64.));
        let label = layer(dim.a);
        if layers.clone().any(|candidate| {
            reads(candidate, MORE_CONTRAST_DISABLED + DISABLED_LABEL_MARGIN)
                && under_ceiling(candidate)
        }) {
            if !under_ceiling(label) {
                failures.push(format!(
                    "{name} disabled label at {} passes 75% of secondary text",
                    dim.a
                ));
            }
        } else if let Some(fainter) = layers
            .clone()
            .find(|candidate| candidate.a < dim.a && reads(*candidate, MORE_CONTRAST_DISABLED))
        {
            failures.push(format!(
                "{name} disabled label at {} where {} reads 4.5:1 nearer the ceiling",
                dim.a, fainter.a
            ));
        }
        let backdrop = applied.dialog_backdrop();
        let dialog = [
            ("dialog", applied.canvas),
            ("dimmed canvas", backdrop_over(applied.canvas, backdrop)),
            ("dimmed panel", backdrop_over(applied.panel, backdrop)),
            ("dimmed subtle", backdrop_over(applied.subtle, backdrop)),
        ];
        let page = more_contrast_edge_surfaces(applied);
        let edges = [
            ("divider", Some(theme.colors.border), &page[..]),
            ("field outline", Some(theme.colors.input), &page[..]),
            ("popup edge", theme.popover_edge, &page[..]),
            ("dialog edge", theme.dialog_edge, &dialog[..]),
            (
                "bordered secondary edge",
                theme.secondary_edge,
                &dialog[..1],
            ),
            ("resting edge", theme.resting_edge, &page[..]),
        ];
        // The kit's resting edge is the adjusted palette's own, its
        // `border`.
        assert_eq!(
            theme.resting_edge,
            Some(hsla(applied.resting_edge())),
            "{name} resting edge"
        );
        assert_eq!(applied.resting_edge(), applied.border, "{name}");
        // A disabled control stays fainter, and a track is a fill.
        assert_eq!(
            (theme.disabled_edge, theme.colors.switch, theme.switch_hover),
            (
                Some(hsla(chosen.disabled_edge())),
                hsla(chosen.border),
                Some(hsla(chosen.switch_hover()))
            ),
            "{name} disabled edge and switch track"
        );
        for (edge, color, surfaces) in edges {
            let Some(color) = color.map(packed) else {
                failures.push(format!("{name} {edge} unset"));
                continue;
            };
            for &(surface, beneath) in surfaces {
                let ratio = contrast(color, beneath);
                if ratio < MORE_CONTRAST_EDGE {
                    failures.push(format!(
                        "{name} {edge} #{color:06x} on {surface} {ratio:.3}:1"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The most sRGB rounding leaves between a syntax role More contrast
/// stops at `text`'s lightness and that lightness, with margin: the
/// largest in every test palette is 0.0012.
const AT_TEXT_LIGHTNESS: f64 = 0.002;

/// The most sRGB rounding moves a role's OKLab opponent coordinates off
/// its palette color's hue, or off the chroma sRGB holds at its
/// lightness, with margin: the largest in every test palette are 0.0014
/// off the hue and 0.0015 off the chroma.
const HUE_ROUNDING: f64 = 0.004;

/// Whether `color` stands at `text`'s OKLab lightness, as a syntax role
/// whose fit stops there under More contrast does.
fn at_text_lightness(color: u32, text: u32) -> bool {
    (oklab(color)[0] - oklab(text)[0]).abs() <= AT_TEXT_LIGHTNESS
}

/// How far `color`'s OKLab opponent coordinates stand from the ray of
/// `source`'s hue, and whether they lie on that ray rather than past
/// grey from it.
fn hue_stray(color: u32, source: u32) -> (f64, bool) {
    let ([_, a, b], [_, source_a, source_b]) = (oklab(color), oklab(source));
    let chroma = source_a.hypot(source_b);
    let (along_a, along_b) = (source_a / chroma, source_b / chroma);
    (
        (a * along_b - b * along_a).abs(),
        a * along_a + b * along_b > 0.,
    )
}

/// The roles More contrast fits in lightness, by name, each with the
/// palette color it draws from and its color in `roles`: every role but
/// comments, which follow secondary text, and `text`.
fn lightness_roles(palette: Palette, roles: SyntaxRoles) -> [(&'static str, u32, u32); 7] {
    [
        ("keyword", palette.renamed, roles.keyword),
        ("string", palette.added, roles.string),
        ("constant", palette.warning, roles.constant),
        ("type", palette.modified, roles.type_),
        ("function", palette.hunk, roles.function),
        ("markup", palette.accent, roles.markup),
        ("removed", palette.removed, roles.removed),
    ]
}

/// The backgrounds syntax is fitted on, by name: the editor background,
/// the active line and both line tints.
fn syntax_backgrounds(palette: Palette) -> [(&'static str, u32); 4] {
    [
        ("editor background", palette.canvas),
        ("active line", palette.panel),
        ("added lines", palette.added_background),
        ("removed lines", palette.removed_background),
    ]
}

/// Under More contrast every syntax role but comments, in every palette,
/// moves only in OKLab lightness, toward `text`'s side, keeping its
/// palette color's hue and chroma, or where sRGB cannot hold that chroma
/// at its lightness the most sRGB holds there, in that hue: it reads
/// 7:1, or as `text` reads where that is lower, on the editor
/// background, the active line and both line tints as More contrast
/// draws them, or stands at `text`'s lightness, and never moves past it.
/// A role whose palette color already reads there is that color, byte
/// for byte, as it is drawn off, and every role reads on each of those
/// backgrounds at least as the role drawn off reads there with More
/// contrast off. Mixed toward `text` in sRGB, the roles spent their
/// chroma and turned their hue, and where `text` reads under 7:1 ended
/// at `text` itself: Omarchy tokyo-night's keyword, function and markup
/// drew #a9b1d6, where they keep their hues at `text`'s lightness
/// (#bfa0f9, #8db2ff, #8db2ff). Unbound by `text`'s lightness, 29 roles
/// passed it by more than rounding, One Dark's keyword by 0.020.
#[test]
fn more_contrast_raises_syntax_in_lightness_keeping_its_hue() {
    let mut failures = Vec::new();
    let (mut kept, mut stopped) = (0, 0);
    for (name, chosen) in more_contrast_palettes() {
        let applied = chosen.with_more_contrast();
        let (off, on) = (
            chosen.syntax_roles(None),
            applied.syntax_roles(Some(chosen)),
        );
        let backgrounds = syntax_backgrounds(applied);
        let reads = |color: u32| {
            backgrounds.iter().all(|&(_, background)| {
                let floor = MORE_CONTRAST_TEXT.min(contrast(applied.text, background));
                contrast(color, background) >= floor
            })
        };
        let text = oklab(applied.text)[0];
        let toward = if luminance(applied.text) > luminance(applied.canvas) {
            1.
        } else {
            -1.
        };
        let drawn_off = lightness_roles(chosen, off).map(|(.., color)| color);
        for ((role, source, fitted), drawn_off) in
            lightness_roles(chosen, on).into_iter().zip(drawn_off)
        {
            let at = format!("{name} {role} #{fitted:06x}");
            for ((surface, background), (_, off_background)) in
                backgrounds.into_iter().zip(syntax_backgrounds(chosen))
            {
                let (is, was) = (
                    contrast(fitted, background),
                    contrast(drawn_off, off_background),
                );
                if is < was {
                    failures.push(format!(
                        "{at} on {surface} {is:.3}:1, #{drawn_off:06x} {was:.3}:1 off"
                    ));
                }
            }
            if reads(source) {
                kept += 1;
                if fitted != source || drawn_off != source {
                    failures.push(format!(
                        "{at} from #{source:06x}, which reads, #{drawn_off:06x} off"
                    ));
                }
                continue;
            }
            let [lightness, a, b] = oklab(fitted);
            let past = (lightness - text) * toward;
            if fitted != source && past > AT_TEXT_LIGHTNESS {
                failures.push(format!("{at} {past:.4} past text's lightness"));
            }
            if !reads(fitted) {
                stopped += 1;
                if past < -AT_TEXT_LIGHTNESS {
                    failures.push(format!(
                        "{at} short of its floor {:.4} before text's lightness",
                        -past
                    ));
                }
            }
            let [_, source_a, source_b] = oklab(source);
            if source_a.hypot(source_b) >= GREY_CHROMA {
                let (stray, along) = hue_stray(fitted, source);
                if stray > HUE_ROUNDING || !along {
                    failures.push(format!("{at} {stray:.4} off #{source:06x}'s hue"));
                }
            }
            let held = oklab(from_linear(linear_in_gamut([
                lightness, source_a, source_b,
            ])));
            let (chroma, held) = (a.hypot(b), held[1].hypot(held[2]));
            if (chroma - held).abs() > HUE_ROUNDING {
                failures.push(format!(
                    "{at} chroma {chroma:.4}, #{source:06x}'s held there {held:.4}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        kept > 0 && stopped > 0,
        "{kept} roles kept, {stopped} stopped at text's lightness"
    );
}

/// Tokyo Night with its added lines lightened until `text` reads 6.25:1
/// there: under More contrast a role reads as `text` reads on those
/// lines and 7:1 elsewhere, short of `text`'s lightness where it can.
/// Its string #9ece6a stops at #aadb76, 0.011 darker than `text`; held
/// to 7:1 on those lines too, it ran on to `text`'s lightness.
#[test]
fn a_role_reads_as_text_does_where_text_reads_under_seven() {
    let palette = Palette {
        added_background: 0x33463c,
        ..ThemeChoice::TokyoNight.palette()
    };
    let on_added = contrast(palette.text, palette.added_background);
    assert!(on_added < MORE_CONTRAST_TEXT, "text {on_added:.3}:1");
    let string = palette.syntax_roles(Some(palette)).string;
    for (surface, background) in syntax_backgrounds(palette) {
        let (ratio, floor) = (
            contrast(string, background),
            MORE_CONTRAST_TEXT.min(contrast(palette.text, background)),
        );
        assert!(
            ratio >= floor,
            "#{string:06x} on {surface} {ratio:.3}:1, needs {floor:.3}"
        );
    }
    let short = oklab(palette.text)[0] - oklab(string)[0];
    assert!(short > 0.005, "#{string:06x} {short:.4} short of text");
    let (stray, along) = hue_stray(string, palette.added);
    assert!(stray <= HUE_ROUNDING && along, "#{string:06x} {stray:.4}");
    assert_eq!(string, 0xaadb76);
}

/// Tokyo Night with its added lines lightened until `text` reads 7.05:1
/// there: its removed #f7768e reaches `text`'s lightness before 7:1 on
/// those lines, so it stops there, #ffb5bf in its own hue at 6.83:1,
/// never lighter than `text`; unbound, it passed `text` by 0.007 to
/// read 7:1.
#[test]
fn a_role_stops_at_text_lightness_where_that_comes_first() {
    let palette = Palette {
        added_background: 0x2c3e34,
        ..ThemeChoice::TokyoNight.palette()
    };
    let on_added = contrast(palette.text, palette.added_background);
    assert!(on_added >= MORE_CONTRAST_TEXT, "text {on_added:.3}:1");
    let removed = palette.syntax_roles(Some(palette)).removed;
    let ratio = contrast(removed, palette.added_background);
    assert!(ratio < MORE_CONTRAST_TEXT, "#{removed:06x} {ratio:.3}:1");
    assert!(
        at_text_lightness(removed, palette.text),
        "#{removed:06x} {:+.4} from text's lightness",
        oklab(removed)[0] - oklab(palette.text)[0]
    );
    let (stray, along) = hue_stray(removed, palette.removed);
    assert!(stray <= HUE_ROUNDING && along, "#{removed:06x} {stray:.4}");
    assert_eq!(removed, 0xffb5bf);
}

/// Tokyo Night with its added lines lightened until `text` reads 4.88:1
/// there: its removed #f7768e stopped at `text`'s lightness, #ffb5bf,
/// reads 4.73:1 on those lines, below the 4.77:1 the role drawn off,
/// #c2c7f1, reads there, so under More contrast it keeps the role drawn
/// off.
#[test]
fn a_role_that_would_read_worse_than_off_keeps_the_role_drawn_off() {
    let palette = Palette {
        added_background: 0x42564c,
        ..ThemeChoice::TokyoNight.palette()
    };
    let (off, on) = (
        palette.syntax_roles(None).removed,
        palette.syntax_roles(Some(palette)).removed,
    );
    let [_, a, b] = oklab(palette.removed);
    let stopped = from_linear(linear_in_gamut([oklab(palette.text)[0], a, b]));
    let line = palette.added_background;
    assert!(
        contrast(stopped, line) < contrast(off, line),
        "#{stopped:06x} {:.3}:1, #{off:06x} {:.3}:1 off",
        contrast(stopped, line),
        contrast(off, line)
    );
    assert_eq!(on, off, "#{on:06x}, #{off:06x} off");
    assert_ne!(on, palette.removed);
}

/// Tokyo Night with its added lines lightened until `text` reads 4.92:1
/// there, then moved darker under More contrast as a line a word cannot
/// leave moves, from #44554a to #405146: each role reads on the editor
/// background, the active line and both line tints at least as the role
/// drawn off reads there on the palette as chosen. Its removed #f7768e,
/// held only to the off fit on the moved line, stopped at `text`'s
/// lightness, #ffb5bf, 10.27:1 on the editor background, where the role
/// drawn off, #c3c6f0, reads 10.30:1, so it keeps that role.
#[test]
fn a_role_reads_as_the_role_drawn_off_on_the_palette_as_chosen() {
    let chosen = Palette {
        added_background: 0x44554a,
        ..ThemeChoice::TokyoNight.palette()
    };
    let adjusted = Palette {
        added_background: 0x405146,
        ..chosen
    };
    let (off, on) = (
        chosen.syntax_roles(None),
        adjusted.syntax_roles(Some(chosen)),
    );
    let drawn_off = lightness_roles(chosen, off).map(|(.., color)| color);
    for ((role, _, fitted), drawn_off) in lightness_roles(chosen, on).into_iter().zip(drawn_off) {
        for ((surface, background), (_, off_background)) in syntax_backgrounds(adjusted)
            .into_iter()
            .zip(syntax_backgrounds(chosen))
        {
            let (is, was) = (
                contrast(fitted, background),
                contrast(drawn_off, off_background),
            );
            assert!(
                is >= was,
                "{role} #{fitted:06x} on {surface} {is:.3}:1, #{drawn_off:06x} {was:.3}:1 off"
            );
        }
    }
    assert_eq!(on.removed, off.removed);
    assert_eq!(off.removed, 0xc3c6f0);
}

/// A word tint's gap from its line in OKLab lightness, signed: positive
/// where the tint is lighter.
fn lightness_gap(tint: u32, line: u32) -> f64 {
    oklab(tint)[0] - oklab(line)[0]
}

/// [`lightness_gap`] as protanopia and deuteranopia see the two colors
/// ([`colour_blind_lightness`]).
fn colour_blind_gaps(tint: u32, line: u32) -> [f64; 2] {
    let [tint, line] = [tint, line].map(colour_blind_lightness);
    [0, 1].map(|model| tint[model] - line[model])
}

/// The colors drawn on a word tint, as `roles` carry them, and `diff`.
fn on_a_word(roles: SyntaxRoles, diff: u32) -> impl Iterator<Item = u32> {
    roles.word_colors().into_iter().chain([diff])
}

/// Of the colors `roles` and `diff` draw on the word tint `on`, the
/// first that reads below its counterpart in `own` on `off`, the tint
/// drawn with More contrast off, as a message.
fn reads_worse_on_a_word(
    (roles, on): (SyntaxRoles, u32),
    (own, off): (SyntaxRoles, u32),
    diff: u32,
) -> Option<String> {
    on_a_word(roles, diff)
        .zip(on_a_word(own, diff))
        .find(|&(color, own)| contrast(color, on) < contrast(own, off))
        .map(|(color, own)| {
            format!(
                "#{color:06x} {:.3}:1 on #{on:06x}, #{own:06x} {:.3}:1 on #{off:06x}",
                contrast(color, on),
                contrast(own, off)
            )
        })
}

/// Under More contrast a word tint nearer its line in OKLab lightness
/// than [`WORD_LIGHTNESS_GAP`] lifts away from it on the side it lies,
/// in synthetic tints on a dark line. Darker than the line under light
/// text, 0.011 from it, the tint lifts darker until the gap reaches
/// 0.04 and stops there, its hue and chroma kept, its luminance contrast
/// with the line growing, and the text reading better. Lighter than the
/// line, 0.011 from it, under light text, the lift lowers the text's
/// contrast, so it stops at the last step where the raised text reads
/// at least as the text drawn off (a partial lift, 0.027), and where the
/// raised text reads no better than off, the tint drawn off stays.
#[test]
fn a_faint_word_tint_lifts_from_its_line_in_lightness() {
    let all = |color: u32| SyntaxRoles {
        keyword: color,
        string: color,
        constant: color,
        type_: color,
        function: color,
        markup: color,
        comment: color,
        removed: color,
        text: color,
    };
    let line = 0x30343a;
    let lifted = |(raised, own): (u32, u32), off: u32, diff: u32| {
        EditorHighlights::lifted_word_tint(all(raised), all(own), off, diff, line)
    };
    let hue = |color: u32| oklab(color)[2].atan2(oklab(color)[1]);
    let chroma = |color: u32| oklab(color)[1].hypot(oklab(color)[2]);

    // Darker than its line, beneath light text: the full lift.
    let (off, text, diff) = (0x1e372c, (0xb4b8be, 0xa0a4aa), 0xa0d0b0);
    assert!((-0.012..-0.01).contains(&lightness_gap(off, line)));
    let tint = lifted(text, off, diff);
    let gap = lightness_gap(tint, line);
    assert!(
        (-0.045..=-WORD_LIGHTNESS_GAP).contains(&gap),
        "#{tint:06x} {gap:+.4} from its line #{line:06x}, off #{off:06x}"
    );
    assert_eq!(lightness_lifts(off, line).last(), Some(&tint));
    assert!((hue(tint) - hue(off)).abs() < 0.05 && (chroma(tint) - chroma(off)).abs() < 0.003);
    assert!(fainter_on_its_line(tint, off, line).is_none());
    assert!(contrast(tint, line) > contrast(off, line));
    assert_eq!(
        reads_worse_on_a_word((all(text.0), tint), (all(text.1), off), diff),
        None
    );

    // Lighter than its line, between it and light text: the partial
    // lift, stopped where the raised text would read worse than off.
    let (off, diff) = (0x233d31, 0x101010);
    assert!((0.01..0.012).contains(&lightness_gap(off, line)));
    let text = (0xb4b8be, 0xaeb2b8);
    let tint = lifted(text, off, diff);
    let gap = lightness_gap(tint, line);
    assert!(
        (0.02..WORD_LIGHTNESS_GAP).contains(&gap),
        "#{tint:06x} {gap:+.4} from its line, off #{off:06x}"
    );
    assert_eq!(
        reads_worse_on_a_word((all(text.0), tint), (all(text.1), off), diff),
        None
    );
    let path = lightness_lifts(off, line);
    let next = path[path.iter().position(|&lift| lift == tint).unwrap() + 1];
    assert!(
        reads_worse_on_a_word((all(text.0), next), (all(text.1), off), diff).is_some(),
        "the step past #{tint:06x}, #{next:06x}, keeps the text"
    );
    assert!(fainter_on_its_line(tint, off, line).is_none());

    // The raised text reads no better than off: no lift keeps it.
    assert_eq!(lifted((0xb4b8be, 0xb4b8be), off, diff), off);

    // Rounding can leave a step further from the line in lightness
    // and luminance but nearer in OKLab than the tint: #1d362c, the
    // first step from #1e372c. Where text reads on that step but not
    // on the next, the lift would stop on it, nearer its line than
    // off; the step is never listed, so the tint drawn off stays.
    let (off, near, line) = (0x1e372c, 0x1d362c, 0x30343a);
    let apart = |tint: u32| oklab_distance(oklab(tint), oklab(line));
    assert!(
        lightness_gap(near, line).abs() > lightness_gap(off, line).abs()
            && contrast(near, line) > contrast(off, line)
            && apart(near) < apart(off)
    );
    // Black text reads on #1d362c at least as #000302 reads off, and
    // not on the next step.
    let (raised, own) = (0x000000, 0x000302);
    let tint = EditorHighlights::lifted_word_tint(all(raised), all(own), off, 0xffffff, line);
    assert_eq!(tint, off, "#{tint:06x}, off #{off:06x}");
    let path = lightness_lifts(off, line);
    assert!(!path.is_empty() && !path.contains(&near));
    assert!(contrast(raised, near) >= contrast(own, off));
    assert!(contrast(raised, path[0]) < contrast(own, off));
}

/// The hue difference of two colors in OKLab, in radians.
fn hue_difference(a: u32, b: u32) -> f64 {
    let hue = |color: u32| oklab(color)[2].atan2(oklab(color)[1]);
    let turn = std::f64::consts::TAU;
    ((hue(a) - hue(b) + turn / 2.).rem_euclid(turn) - turn / 2.).abs()
}

/// Where sRGB cannot hold a lifted tint's chroma at its new lightness,
/// near black and near white, the lift lowers the chroma and keeps the
/// hue: in a near-black green (#00351d on #19322d) and a near-white one
/// (#e8f4c5 on #efeedc), every step stays within 0.03 radians of the
/// tint's hue and never gains chroma beyond rounding, and the last
/// reaches the gap with its chroma lowered. Clipped to sRGB instead,
/// the steps turned up to 0.057 and 0.091 radians.
#[test]
fn a_word_tint_lift_keeps_its_hue_where_the_gamut_takes_chroma() {
    let chroma = |color: u32| oklab(color)[1].hypot(oklab(color)[2]);
    for (off, line) in [(0x00351d, 0x19322d), (0xe8f4c5, 0xefeedc)] {
        let path = lightness_lifts(off, line);
        let last = *path.last().expect("a lift");
        assert!(lightness_gap(last, line).abs() >= WORD_LIGHTNESS_GAP);
        assert!(
            chroma(last) < chroma(off) - 0.005,
            "#{last:06x} chroma {:.4}, off #{off:06x} {:.4}: the gamut binds",
            chroma(last),
            chroma(off)
        );
        for step in path {
            assert!(
                hue_difference(step, off) < 0.03 && chroma(step) <= chroma(off) + 0.0015,
                "#{step:06x} turned {:.4} from #{off:06x}, chroma {:.4} from {:.4}",
                hue_difference(step, off),
                chroma(step),
                chroma(off)
            );
        }
    }
}

/// A lift that runs out of lightness stops at the last step before
/// white: on a line #f5f3f0, from which white stands 0.035 in OKLab
/// lightness, a tint #f9f8f6 lighter than it under dark text lifts to
/// the end of its path, short of the gap, its text reading better.
#[test]
fn a_word_tint_lift_stops_where_lightness_runs_out() {
    let (off, line, text) = (0xf9f8f6, 0xf5f3f0, 0x202020);
    assert!(lightness_gap(0xffffff, line) < WORD_LIGHTNESS_GAP);
    let roles = SyntaxRoles {
        keyword: text,
        string: text,
        constant: text,
        type_: text,
        function: text,
        markup: text,
        comment: text,
        removed: text,
        text,
    };
    let tint = EditorHighlights::lifted_word_tint(roles, roles, off, text, line);
    assert_eq!(lightness_lifts(off, line).last(), Some(&tint));
    let gap = lightness_gap(tint, line);
    assert!(
        gap > lightness_gap(off, line) + 0.015 && gap < WORD_LIGHTNESS_GAP,
        "#{tint:06x} {gap:+.4} from its line, off #{off:06x}"
    );
    assert!(contrast(tint, line) > contrast(off, line));
    assert!(contrast(text, tint) > contrast(text, off));
}

/// How a highlight drawn under More contrast, `on`, marks itself on
/// `line` more faintly than `off`, the one drawn with it off, if it
/// does: by a lower luminance contrast with the line, or on the other
/// side of it in luminance. A highlight's lightness against its line is
/// the mark that survives colour-blind vision; one that stood apart
/// through saturation alone took its line's luminance (every raised word
/// tint 1.00 to 1.01:1, where the ones drawn off stand 1.08 to 1.37:1).
fn fainter_on_its_line(on: u32, off: u32, line: u32) -> Option<String> {
    let side = |tint: u32| custom::luminance(tint).total_cmp(&custom::luminance(line));
    let (ratio, off_ratio) = (contrast(on, line), contrast(off, line));
    let crossed = side(off).is_ne() && side(on) != side(off);
    (ratio < off_ratio || crossed).then(|| {
        format!(
            "#{on:06x} {ratio:.3}:1 on the line #{line:06x}, {:?} it in luminance, off \
             #{off:06x} {off_ratio:.3}:1, {:?}",
            side(on),
            side(off)
        )
    })
}

/// With More contrast on, no color reads worse on a changed word than it
/// does with More contrast off and no changed word is marked more
/// faintly, and the faintest are lifted from their lines, in every
/// palette. On the added and removed word tints the editors draw, `text`,
/// every syntax role but `removed` and the tint's diff color each read
/// at least the contrast the same role reads on the tint drawn with it
/// off; each tint stands from its line tint, in OKLab and in luminance
/// contrast, at least as far as the tint drawn off stands from the line
/// drawn off, and on the same side of it, the line moved or not, and its
/// lightness gap from the line drawn off in a protanopia and
/// deuteranopia model is no smaller than off. A tint
/// drawn off at least [`WORD_LIGHTNESS_GAP`] from its line in OKLab
/// lightness stays exactly; a nearer one stands at least that far from
/// the line drawn off, or is the last step of its lift
/// ([`lightness_lifts`]) before one on which some color would read worse
/// than off, or before lightness runs out, or has no lift, lying on one
/// side of its line in lightness and on the other in luminance. These
/// hold for every word drawn off beyond its line from the canvas
/// ([`lightness_beyond`]); a word drawn off on its line's canvas side
/// steps across the line instead, and its place is read in
/// `more_contrast_keeps_the_highlight_rules_in_every_palette`. Re-fitted
/// at 4.75:1
/// against syntax raised to 7:1, the added word tint grew heavier, and
/// the added text on it read 5.81:1 where it read 6.45:1 (Midnight);
/// held only to the readings, it drew nearer its line (Midnight's added
/// words 0.077 to 0.041 from it, removed 0.094 to 0.056); held to the
/// distance alone, it could cross to the line's other side (Midnight's
/// added words #0f1d19, darker than their line #19322d, where the tint
/// drawn off is lighter); lifted toward the canvas, Omarchy
/// flexoki-light's added word reached #f7ffdf, 1.005:1 against its
/// canvas, and held at half its line's gap from the canvas, 31 words
/// stayed on their line's canvas side.
#[gpui::test]
fn more_contrast_never_reads_worse_on_a_changed_word(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    const ROLES: [&str; 9] = [
        "keyword", "string", "constant", "type", "function", "markup", "comment", "text", "diff",
    ];
    let mut failures = Vec::new();
    let (mut lifted, mut bound, mut level, mut edged, mut kept) = (0, 0, 0, 0, 0);
    let mut crossed = 0;
    for (name, chosen) in more_contrast_palettes() {
        let [off, on] = [false, true].map(|more| {
            cx.update(|cx| {
                set_more_contrast(more, cx);
                chosen.apply(chosen.is_light(), None, cx);
                let applied = palette(cx);
                (
                    applied,
                    editor_highlights(cx),
                    applied.syntax_roles(more.then_some(chosen)),
                )
            })
        });
        let ((before, off_tints, off_roles), (after, on_tints, on_roles)) = (off, on);
        // The syntax the lift is judged under: raised on the line tints
        // as chosen, before any line moves.
        let lift_roles = chosen.with_raised_floors().syntax_roles(Some(chosen));
        for (tint, diff, (off_line, on_line), off_tint, on_tint) in [
            (
                "added",
                after.added,
                (before.added_background, after.added_background),
                off_tints.added_word,
                on_tints.added_word,
            ),
            (
                "removed",
                after.removed,
                (before.removed_background, after.removed_background),
                off_tints.removed_word,
                on_tints.removed_word,
            ),
        ] {
            // A word drawn off on its line's canvas side steps across the
            // line instead ([`EditorHighlights::crossed_word_tint`]).
            if lightness_beyond(off_tint, off_line, chosen) <= 0. {
                crossed += 1;
                continue;
            }
            let colors = |roles: SyntaxRoles| on_a_word(roles, diff);
            for ((role, was), is) in ROLES.iter().zip(colors(off_roles)).zip(colors(on_roles)) {
                let (off_ratio, on_ratio) = (contrast(was, off_tint), contrast(is, on_tint));
                if on_ratio < off_ratio {
                    failures.push(format!(
                        "{name} {role} on the {tint} word #{on_tint:06x} {on_ratio:.3}:1, \
                         off #{off_tint:06x} {off_ratio:.3}:1"
                    ));
                }
            }
            let gap = |tint: u32| lightness_gap(tint, off_line).abs();
            if gap(off_tint) >= WORD_LIGHTNESS_GAP {
                if on_tint != off_tint {
                    failures.push(format!(
                        "{name} {tint} word #{on_tint:06x}, off #{off_tint:06x} {:.4} from \
                         its line in lightness",
                        gap(off_tint)
                    ));
                }
                kept += 1;
            } else if gap(on_tint) >= WORD_LIGHTNESS_GAP {
                lifted += 1;
            } else {
                // Short of the gap, the next step of the lift would read
                // some color worse than off; or the tint has no lift: it lies on one
                // side of its line in lightness and on the other in
                // luminance, so any step away in lightness would near the
                // line in luminance or cross it; or the lift ran out of
                // lightness, white or black on its side standing nearer
                // the line than the gap.
                let path = lightness_lifts(off_tint, off_line);
                let next = match path.iter().position(|&lift| lift == on_tint) {
                    Some(at) => path.get(at + 1),
                    None => path.first().filter(|_| on_tint == off_tint),
                };
                let disagrees = lightness_gap(off_tint, off_line).signum()
                    != (luminance(off_tint) - luminance(off_line)).signum();
                let edge = if lightness_gap(off_tint, off_line) > 0. {
                    0xffffff
                } else {
                    0x000000
                };
                let ran_out = path.last() == Some(&on_tint) && gap(edge) < WORD_LIGHTNESS_GAP;
                match next {
                    Some(&next)
                        if reads_worse_on_a_word(
                            (lift_roles, next),
                            (off_roles, off_tint),
                            diff,
                        )
                        .is_some() =>
                    {
                        bound += 1;
                    }
                    None if path.is_empty() && on_tint == off_tint && disagrees => {
                        level += 1;
                    }
                    None if ran_out => edged += 1,
                    _ => failures.push(format!(
                        "{name} {tint} word #{on_tint:06x} {:.4} from its line #{off_line:06x} \
                         in lightness, off #{off_tint:06x} {:.4}, short of the gap, not \
                         where the text binds and not at the end of lightness (next step \
                         {next:x?})",
                        gap(on_tint),
                        gap(off_tint)
                    )),
                }
            }
            let apart = |tint: u32, line: u32| oklab_distance(oklab(tint), oklab(line));
            if apart(on_tint, on_line) < apart(off_tint, off_line) {
                failures.push(format!(
                    "{name} {tint} word #{on_tint:06x} {:.3} from its line #{on_line:06x}, \
                     off #{off_tint:06x} {:.3} from #{off_line:06x}",
                    apart(on_tint, on_line),
                    apart(off_tint, off_line)
                ));
            }
            let side = |tint: u32, line: u32| oklab(tint)[0].total_cmp(&oklab(line)[0]);
            if side(on_tint, on_line) != side(off_tint, off_line) {
                failures.push(format!(
                    "{name} {tint} word #{on_tint:06x} {:?} its line #{on_line:06x} in \
                     lightness, off #{off_tint:06x} {:?} #{off_line:06x}",
                    side(on_tint, on_line),
                    side(off_tint, off_line)
                ));
            }
            let luminance_side = |tint: u32, line: u32| luminance(tint).total_cmp(&luminance(line));
            let off_side = luminance_side(off_tint, off_line);
            if contrast(on_tint, on_line) < contrast(off_tint, off_line)
                || (off_side.is_ne() && luminance_side(on_tint, on_line) != off_side)
            {
                failures.push(format!(
                    "{name} {tint} word #{on_tint:06x} {:.3}:1 on its line #{on_line:06x}, \
                     {:?} it in luminance, off #{off_tint:06x} {:.3}:1 on #{off_line:06x}, \
                     {off_side:?}",
                    contrast(on_tint, on_line),
                    luminance_side(on_tint, on_line),
                    contrast(off_tint, off_line)
                ));
            }
            // The lift's own gap in colour-blind vision, from the line
            // drawn off; a moved line is held to normal vision.
            let seen = colour_blind_gaps(on_tint, off_line)
                .into_iter()
                .zip(colour_blind_gaps(off_tint, off_line));
            for (vision, (on, off)) in ["protanopia", "deuteranopia"].into_iter().zip(seen) {
                if on.abs() < off.abs() {
                    failures.push(format!(
                        "{name} {tint} word #{on_tint:06x} {on:+.4} from its line in \
                         {vision}, off #{off_tint:06x} {off:+.4}"
                    ));
                }
            }
        }
    }
    cx.update(|cx| set_more_contrast(false, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Every case a palette reaches is reached: tints kept, stopped where
    // the text binds, with no lift, and stepped across their lines. No
    // palette's word drawn off beyond its line lifts to the gap or to
    // the end of lightness (every lift that reached the gap went toward
    // the canvas); `a_faint_word_tint_lifts_from_its_line_in_lightness`
    // and `a_word_tint_lift_stops_where_lightness_runs_out` read those.
    assert!(
        kept > 0 && bound > 0 && level > 0 && crossed > 0,
        "word tints kept {kept}, lifted {lifted}, at the text's bound {bound}, with no lift \
         {level}, at the end of lightness {edged}, stepped across {crossed}"
    );
}

/// Of the colors drawn on a diff side's line and word tints under More
/// contrast, `after` with `on_roles`, the first that reads below the same
/// color drawn with More contrast off, `before` with `off_roles`, as a
/// message: on the line `text`, every syntax role, the side's diff color,
/// secondary text and line numbers; on the word `text`, every role but
/// `removed` and the diff color.
fn diff_side_reads_worse(
    (after, on_roles, on_line, on_word): (Palette, SyntaxRoles, u32, u32),
    (before, off_roles, off_line, off_word): (Palette, SyntaxRoles, u32, u32),
    diff: u32,
) -> Option<String> {
    let on_line_colors = |palette: Palette, roles: SyntaxRoles| {
        roles
            .colors()
            .into_iter()
            .chain([diff, palette.muted, palette.line_number])
    };
    let line = on_line_colors(after, on_roles)
        .zip(on_line_colors(before, off_roles))
        .map(|pair| (pair, (on_line, off_line)));
    let word = on_a_word(on_roles, diff)
        .zip(on_a_word(off_roles, diff))
        .map(|pair| (pair, (on_word, off_word)));
    line.chain(word)
        .find(|&((is, was), (on, off))| contrast(is, on) < contrast(was, off))
        .map(|((is, was), (on, off))| {
            format!(
                "#{is:06x} {:.3}:1 on #{on:06x}, off #{was:06x} {:.3}:1 on #{off:06x}",
                contrast(is, on),
                contrast(was, off)
            )
        })
}

/// Why `rule` is not the rule a changed word whose tint is `word` draws
/// for the diff colour `diff` in a light or dark palette
/// ([`EditorHighlights::word_rule`]), or `None` where it is: it reads
/// [`MORE_CONTRAST_EDGE`] on the tint, it is `diff` where `diff` already
/// reads so, and otherwise it is one of 256 even steps from `diff`
/// toward black in a light palette or white in a dark one, the step
/// before it reading under 3:1.
fn word_rule_misses(light: bool, diff: u32, word: u32, rule: u32) -> Option<String> {
    let (ratio, diff_ratio) = (contrast(rule, word), contrast(diff, word));
    if ratio < MORE_CONTRAST_EDGE {
        return Some(format!(
            "#{rule:06x} reads {ratio:.3}:1 on its word #{word:06x}"
        ));
    }
    if diff_ratio >= MORE_CONTRAST_EDGE {
        return (rule != diff).then(|| {
            format!(
                "#{rule:06x} leaves its diff colour #{diff:06x}, which reads \
                 {diff_ratio:.3}:1 on its word #{word:06x}"
            )
        });
    }
    let extreme = if light { 0x000000 } else { 0xffffff };
    let step = |k: u32| {
        [16, 8, 0].into_iter().fold(0, |color, shift| {
            let (from, to) = ((diff >> shift) & 0xff, (extreme >> shift) & 0xff);
            color | (((from * (256 - k) + to * k) / 256) << shift)
        })
    };
    let first = (1..=256)
        .find(|&k| step(k) == rule)
        .is_some_and(|k| contrast(step(k - 1), word) < MORE_CONTRAST_EDGE);
    (!first).then(|| {
        format!(
            "#{rule:06x} is not the first step from #{diff:06x} toward #{extreme:06x} that \
             reads 3:1 on its word #{word:06x}"
        )
    })
}

/// Under More contrast every changed word, in every palette and on both
/// sides, draws a rule along its highlight box's bottom edge
/// ([`EditorHighlights::word_rule`]) that reads [`MORE_CONTRAST_EDGE`]
/// on the word's tint as applied, its side's diff colour where that
/// already reads so ([`word_rule_misses`]). Off, no word draws one, so the
/// decorations are those drawn before the rule existed. Every shipped
/// palette's diff colours read 3:1 on their word tints, which are fitted
/// for them as text (the least, Kanagawa Wave's removed `#ff5d62` on
/// `#5c0c1c`, 4.59:1, measured on 2026-11-05), so each draws its diff
/// colour; `a_word_rule_steps_from_a_diff_colour_under_three_to_one`
/// reads the step where one would not.
#[gpui::test]
fn more_contrast_rules_every_changed_word_at_three_to_one(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let mut failures = Vec::new();
    for (name, chosen) in more_contrast_palettes() {
        let off = cx.update(|cx| {
            set_more_contrast(false, cx);
            chosen.apply(chosen.is_light(), None, cx);
            editor_highlights(cx)
        });
        if off.added_rule.is_some() || off.removed_rule.is_some() {
            failures.push(format!(
                "{name} rules a changed word with More contrast off"
            ));
        }
        let (applied, on) = cx.update(|cx| {
            set_more_contrast(true, cx);
            chosen.apply(chosen.is_light(), None, cx);
            (palette(cx), editor_highlights(cx))
        });
        for (side, diff, word, rule) in [
            ("added", applied.added, on.added_word, on.added_rule),
            ("removed", applied.removed, on.removed_word, on.removed_rule),
        ] {
            match rule {
                None => failures.push(format!("{name}'s {side} word draws no rule")),
                Some(rule) => failures.extend(
                    word_rule_misses(applied.is_light(), diff, word, rule)
                        .map(|miss| format!("{name}'s {side} rule {miss}")),
                ),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A diff colour that reads under 3:1 on its word's tint gives the rule
/// the first of 256 steps from it toward white in a dark palette or black
/// in a light one that reads 3:1 there, and one that reads 3:1 is kept.
/// No shipped palette has such a pair, so these are made: Midnight's
/// removed word tint under a dim rose, and Daylight's added word tint
/// under a pale green.
#[test]
fn a_word_rule_steps_from_a_diff_colour_under_three_to_one() {
    for (choice, diff, word) in [
        (ThemeChoice::Midnight, 0x8a4a5c, 0x5f3241),
        (ThemeChoice::Midnight, 0xff95a8, 0x5f3241),
        (ThemeChoice::Daylight, 0x9fd8a8, 0xd6f5da),
        (ThemeChoice::Daylight, 0x1a7f37, 0xd6f5da),
    ] {
        let palette = choice.palette();
        let rule = EditorHighlights::word_rule(palette, diff, word);
        assert_eq!(
            word_rule_misses(palette.is_light(), diff, word, rule),
            None,
            "{choice:?}: #{diff:06x} on #{word:06x}"
        );
        assert_eq!(
            rule == diff,
            contrast(diff, word) >= MORE_CONTRAST_EDGE,
            "{choice:?}: #{diff:06x} on #{word:06x} moves only where it misses 3:1"
        );
    }
}

/// Under More contrast a changed word that cannot stand
/// [`WORD_LIGHTNESS_GAP`] from its line in OKLab lightness moves its line
/// instead, in every palette ([`Palette::more_contrast_fit`]). A side
/// whose word is drawn off on its line's canvas side keeps its line, and
/// its word steps across it (read in
/// `more_contrast_keeps_the_highlight_rules_in_every_palette`). On every
/// other side the line moves for the word's lift
/// ([`EditorHighlights::lifted_word_tint`]), which is the word drawn, or
/// where it does not stand beyond the moved line, steps across it too. A
/// side whose lift stands that far from the line drawn off keeps that
/// line. Every
/// other side stands that far from its moved line, or has its line at a
/// bound: the next step of its move ([`line_moves`]) would stand nearer
/// the selection or Find's matches drawn off than they allow, narrow the
/// word's lightness gap in a protanopia or deuteranopia model, or read
/// some color on the diff's lines or words worse than off; or the move
/// ended at the line's 0.04 from the canvas, or has no step, the line
/// standing within 0.04 of the canvas on the side it would move toward,
/// or the word lying on one side of it in lightness and the other in
/// luminance. On every side every color drawn on line and the word the
/// line moves for reads at least as off (the word's own place is read in
/// `more_contrast_never_reads_worse_on_a_changed_word`). A moved line
/// stands 0.04 from the canvas in lightness, or no nearer than off, the
/// selection and Find's current match drawn off stand 0.04 from it, or as
/// far as from the line drawn off, the word stands from it in a
/// protanopia and a deuteranopia model at least as far as from the line
/// drawn off, and secondary text and line numbers read 7:1 on it, or as
/// `text` does.
#[gpui::test]
fn more_contrast_moves_a_line_its_word_cannot_stand_apart_from(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let mut failures = Vec::new();
    let (mut full, mut floored, mut within, mut text_bound, mut apart_bound, mut level) =
        (0, 0, 0, 0, 0, 0);
    let (mut colour_blind_bound, mut kept_by_crossing) = (0, 0);
    for (name, chosen) in more_contrast_palettes() {
        let [off, on] = [false, true].map(|more| {
            cx.update(|cx| {
                set_more_contrast(more, cx);
                chosen.apply(chosen.is_light(), None, cx);
                let applied = palette(cx);
                (
                    applied,
                    editor_highlights(cx),
                    applied.syntax_roles(more.then_some(chosen)),
                )
            })
        });
        let ((before, off_tints, off_roles), (after, on_tints, on_roles)) = (off, on);
        let canvas = oklab(chosen.canvas)[0];
        let from_canvas = |color: u32| (oklab(color)[0] - canvas).abs();
        let off_words = [off_tints.added_word, off_tints.removed_word];
        let on_words = [on_tints.added_word, on_tints.removed_word];
        let sides = [
            (
                "added",
                chosen.added,
                (before.added_background, after.added_background),
            ),
            (
                "removed",
                chosen.removed,
                (before.removed_background, after.removed_background),
            ),
        ];
        let fit_words = words_lines_move_for(chosen);
        let crossing =
            [0, 1].map(|index| lightness_beyond(off_words[index], sides[index].2.0, chosen) <= 0.);
        let gap = |word: u32, line: u32| lightness_gap(word, line).abs();
        let drawn = |palette: Palette, roles: SyntaxRoles, line: u32, word: u32| {
            (palette, roles, line, word)
        };
        // The bound a step of side `index`'s move fails, if any: the
        // highlights' distance, the word's colour-blind gaps or a color
        // on the diff's lines and words reading worse than off, measured
        // as the fit measures it, the added line moving beside the
        // removed line drawn off.
        let failing_bound = |index: usize, step: u32| -> Option<&'static str> {
            let off_line = sides[index].2.0;
            let off_line_lab = oklab(off_line);
            let nearer = [off_tints.selection, off_tints.find_current]
                .into_iter()
                .any(|highlight| {
                    let highlight = oklab(highlight);
                    oklab_distance(highlight, oklab(step))
                        < HIGHLIGHT_DISTANCE.min(oklab_distance(highlight, off_line_lab))
                });
            let narrows = colour_blind_gaps(fit_words[index], step)
                .into_iter()
                .zip(colour_blind_gaps(fit_words[index], off_line))
                .any(|(moved, off)| moved.abs() < off.abs());
            let mut lines = [after.added_background, after.removed_background];
            if index == 0 {
                lines[1] = before.removed_background;
            }
            lines[index] = step;
            let candidate = Palette {
                added_background: lines[0],
                removed_background: lines[1],
                ..chosen
            }
            .with_raised_floors();
            let roles = candidate.syntax_roles(Some(chosen));
            let worse = || {
                sides.iter().enumerate().any(|(at, &(_, diff, (off, _)))| {
                    diff_side_reads_worse(
                        drawn(candidate, roles, lines[at], fit_words[at]),
                        drawn(before, off_roles, off, off_words[at]),
                        diff,
                    )
                    .is_some()
                })
            };
            if nearer {
                Some("highlights")
            } else if narrows {
                Some("colour-blind")
            } else if worse() {
                Some("text")
            } else {
                None
            }
        };
        for (index, &(side, diff, (off_line, on_line))) in sides.iter().enumerate() {
            let (off_word, on_word) = (off_words[index], fit_words[index]);
            if let Some(worse) = diff_side_reads_worse(
                drawn(after, on_roles, on_line, on_word),
                drawn(before, off_roles, off_line, off_word),
                diff,
            ) {
                failures.push(format!("{name} {side}: {worse}"));
            }
            if crossing[index] {
                if on_line != off_line {
                    failures.push(format!(
                        "{name} {side} line #{off_line:06x} moved to #{on_line:06x} for a \
                         word on its canvas side #{off_word:06x}"
                    ));
                }
                kept_by_crossing += 1;
                continue;
            }
            // The word drawn is the lift, or steps across a line it does
            // not stand beyond.
            let drawn_word = on_words[index];
            if drawn_word != on_word && stands_beyond(on_word, on_line, chosen) {
                failures.push(format!(
                    "{name} {side} word #{drawn_word:06x}, its lift #{on_word:06x} stands \
                     beyond its line #{on_line:06x}"
                ));
            }
            if gap(on_word, off_line) >= WORD_LIGHTNESS_GAP && on_line != off_line {
                failures.push(format!(
                    "{name} {side} line #{off_line:06x} moved to #{on_line:06x}, its word \
                     #{on_word:06x} {:.4} from it",
                    gap(on_word, off_line)
                ));
            }
            if on_line != off_line {
                if from_canvas(on_line) < WORD_LIGHTNESS_GAP.min(from_canvas(off_line)) {
                    failures.push(format!(
                        "{name} {side} line #{on_line:06x} {:.4} from the canvas, off \
                         #{off_line:06x} {:.4}",
                        from_canvas(on_line),
                        from_canvas(off_line)
                    ));
                }
                for (highlight, color) in [
                    ("selection", off_tints.selection),
                    ("current Find match", off_tints.find_current),
                ] {
                    let apart = |line: u32| oklab_distance(oklab(color), oklab(line));
                    if apart(on_line) < HIGHLIGHT_DISTANCE.min(apart(off_line)) {
                        failures.push(format!(
                            "{name} {side} line #{on_line:06x} {:.4} from the {highlight} \
                             drawn off #{color:06x}, off #{off_line:06x} {:.4}",
                            apart(on_line),
                            apart(off_line)
                        ));
                    }
                }
                let seen = colour_blind_gaps(on_word, on_line)
                    .into_iter()
                    .zip(colour_blind_gaps(on_word, off_line));
                for (vision, (on, off)) in ["protanopia", "deuteranopia"].into_iter().zip(seen) {
                    if on.abs() < off.abs() {
                        failures.push(format!(
                            "{name} {side} word #{on_word:06x} {on:+.4} from its moved line \
                             #{on_line:06x} in {vision}, {off:+.4} from #{off_line:06x}"
                        ));
                    }
                }
                let floor = MORE_CONTRAST_TEXT.min(contrast(after.text, on_line));
                for (token, color) in [("muted", after.muted), ("line_number", after.line_number)] {
                    if contrast(color, on_line) < floor {
                        failures.push(format!(
                            "{name} {token} #{color:06x} {:.3}:1 on the moved {side} line \
                             #{on_line:06x}, needs {floor:.3}",
                            contrast(color, on_line)
                        ));
                    }
                }
            }
            let moves = line_moves(off_line, on_word, (off_word, off_line), chosen.canvas);
            // No step up to the line drawn fails a bound: the move never
            // passes a step that would, even where a later one would not.
            if let Some(at) = moves.iter().position(|&line| line == on_line) {
                for &step in &moves[..=at] {
                    if let Some(bound) = failing_bound(index, step) {
                        failures.push(format!(
                            "{name} {side} line #{on_line:06x} passed #{step:06x}, which \
                             fails the {bound} bound"
                        ));
                    }
                }
            }
            if gap(on_word, on_line) >= WORD_LIGHTNESS_GAP {
                full += usize::from(on_line != off_line);
                continue;
            }
            let next = match moves.iter().position(|&line| line == on_line) {
                Some(at) => moves.get(at + 1).copied(),
                None if on_line == off_line => moves.first().copied(),
                None => {
                    failures.push(format!(
                        "{name} {side} line #{on_line:06x} is no step of its move {moves:x?}"
                    ));
                    continue;
                }
            };
            let disagrees = lightness_gap(on_word, off_line).signum()
                != (luminance(on_word) - luminance(off_line)).signum();
            let toward_canvas = (oklab(off_line)[0] - oklab(on_word)[0]).signum()
                == (canvas - oklab(off_line)[0]).signum();
            match next {
                Some(next) => match failing_bound(index, next) {
                    Some("highlights") => apart_bound += 1,
                    Some("colour-blind") => colour_blind_bound += 1,
                    Some(_) => text_bound += 1,
                    None => failures.push(format!(
                        "{name} {side} line #{on_line:06x} stopped short of #{next:06x}, its \
                         word #{on_word:06x} {:.4} from it, where every bound holds",
                        gap(on_word, on_line)
                    )),
                },
                None if moves.is_empty() && disagrees => level += 1,
                None if moves.is_empty()
                    && toward_canvas
                    && from_canvas(off_line) < WORD_LIGHTNESS_GAP =>
                {
                    within += 1;
                }
                // The move ended at the canvas floor: the line stands
                // within a step and its rounding of 0.04 from the canvas.
                None if toward_canvas && from_canvas(on_line) < WORD_LIGHTNESS_GAP + 0.004 => {
                    floored += 1;
                }
                None => failures.push(format!(
                    "{name} {side} line #{on_line:06x}, its word #{on_word:06x} {:.4} from \
                     it, ended its move {moves:x?} at no bound",
                    gap(on_word, on_line)
                )),
            }
        }
    }
    cx.update(|cx| set_more_contrast(false, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Each bound is reached by a built-in palette but the selection's,
    // which the Omarchy palettes and a synthetic case reach
    // (`a_moved_line_stops_at_its_bounds`), and the text's: a line moves
    // away from its word, toward the canvas, where its text reads better,
    // and the text stopped only lines that would move toward it, for
    // words on their canvas side, which keep their lines now. The
    // colour-blind gaps stop One Dark's and Dracula's removed lines.
    assert!(
        full > 0
            && floored > 0
            && within > 0
            && colour_blind_bound > 0
            && level > 0
            && kept_by_crossing > 0,
        "lines moved to the gap {full}, at the canvas floor {floored}, within it {within}, \
         at the text's bound {text_bound}, the highlights' {apart_bound}, the colour-blind \
         gaps' {colour_blind_bound}, with no move {level}, kept for a word that steps \
         across {kept_by_crossing}"
    );
}

/// The diff tints [`Palette::more_contrast_fit`] decides for a palette,
/// with the editor highlights drawn off: the adjusted palette, the word
/// tints under More contrast and those drawn off, added then removed.
fn diff_tints(chosen: Palette) -> (Palette, [u32; 2], [u32; 2]) {
    let (adjusted, fit) = chosen.more_contrast_fit();
    let off = EditorHighlights::fit(chosen, chosen.syntax_roles(None), None);
    (adjusted, fit.words, [off.added_word, off.removed_word])
}

/// The word tints [`Palette::more_contrast_fit`] moves `chosen`'s line
/// tints for, added then removed: each drawn with More contrast off
/// beyond its line from the canvas lifted
/// ([`EditorHighlights::lifted_word_tint`]), and one on its line's
/// canvas side as drawn off, since it steps across its line afterward.
fn words_lines_move_for(chosen: Palette) -> [u32; 2] {
    let (_, _, off) = diff_tints(chosen);
    let roles = chosen.with_raised_floors().syntax_roles(Some(chosen));
    let own = chosen.syntax_roles(None);
    [
        (chosen.added, chosen.added_background),
        (chosen.removed, chosen.removed_background),
    ]
    .into_iter()
    .zip(off)
    .map(|((diff, line), off)| {
        if lightness_beyond(off, line, chosen) > 0. {
            EditorHighlights::lifted_word_tint(roles, own, off, diff, line)
        } else {
            off
        }
    })
    .collect::<Vec<_>>()
    .try_into()
    .expect("two sides")
}

/// Whether the text rule keeps a word that steps across `line` short of
/// [`HIGHLIGHT_LIGHTNESS_GAP`] beyond it: in each hue
/// [`EditorHighlights::crossed_word_tint`] steps in, the seed's of `diff`
/// on `line`, halfway to `diff`'s and `diff`'s own, the first step that
/// stands [`HIGHLIGHT_DISTANCE`] from the line and the gap beyond it
/// reads some color in `colors` under the text rule, or there is none.
fn the_gap_cannot_read(colors: &[u32], diff: u32, line: u32, chosen: Palette) -> bool {
    let seed = EditorHighlights::word_seed(diff, line);
    [seed, mix(seed, diff, 128), diff].into_iter().all(|hue| {
        beyond_line(hue, line, chosen)
            .into_iter()
            .find(|&step| {
                lightness_beyond(step, line, chosen) >= HIGHLIGHT_LIGHTNESS_GAP
                    && oklab_distance(oklab(step), oklab(line)) >= HIGHLIGHT_DISTANCE
            })
            .is_none_or(|step| {
                colors
                    .iter()
                    .any(|&color| contrast(color, step) < LABEL_RULE)
            })
    })
}

/// A color's OKLab lightness gap from `palette`'s canvas.
fn from_canvas(palette: Palette, color: u32) -> f64 {
    (oklab(color)[0] - oklab(palette.canvas)[0]).abs()
}

/// Under More contrast a word tint drawn off on its line's canvas side
/// never lifts toward the canvas: its line keeps its tint and the word
/// steps across it ([`EditorHighlights::crossed_word_tint`]), in
/// built-in palettes. It stands beyond the line from the canvas, at least
/// [`HIGHLIGHT_DISTANCE`] from it and [`HIGHLIGHT_LIGHTNESS_GAP`] beyond
/// it in lightness, and every color drawn on it as text reads the text
/// rule; One Dark's added word stops short of the gap, beyond its line,
/// since at the gap its raised keyword would read 4.39:1, under the text
/// rule (Kanagawa Lotus's removed word stopped short too, its strings
/// 4.49:1 at the gap, until its text darkened on 2026-11-05). It is the
/// furthest such step in its row of steps ([`beyond_line`]): the next
/// step in the row reads some color below the rule the word keeps, or
/// the row ends there (Solarized Dark's added `#214514`, One Dark's added
/// `#2f491d`, Kanagawa Lotus's removed `#f7bea6`, where it drew `#ffbdb4`
/// 0.018 beyond its line). One Dark's added word, drawn off darker than its line, once
/// lifted toward the canvas and stopped at half its line's gap from it
/// (#253b23), still on the canvas side; Rosé Pine Dawn's removed word,
/// drawn off 0.001 from its canvas, kept the tint drawn off; Braden's
/// removed word (#fcf6f7 on #f9e5eb), lighter than its canvas, lifted
/// away from it, past the canvas from its line. Solarized Dark's added
/// word, held to the distance from its line alone, stepped across
/// through its seed's chroma to #2d3500, 0.0004 beyond its line in
/// lightness.
#[test]
fn a_word_on_its_lines_canvas_side_steps_across_it() {
    for (case, theme, side, at_gap, pinned) in [
        (
            "One Dark added",
            ThemeChoice::OneDark,
            0,
            false,
            Some(0x2f491d),
        ),
        (
            "Rosé Pine Dawn removed",
            ThemeChoice::RosePineDawn,
            1,
            true,
            None,
        ),
        ("Braden removed", ThemeChoice::Daylight, 1, true, None),
        (
            "Solarized Dark added",
            ThemeChoice::SolarizedDark,
            0,
            true,
            Some(0x214514),
        ),
        (
            "Kanagawa Lotus removed",
            ThemeChoice::KanagawaLotus,
            1,
            true,
            Some(0xf7bea6),
        ),
    ] {
        let chosen = theme.palette();
        let (adjusted, words, off) = diff_tints(chosen);
        let roles = adjusted.syntax_roles(Some(chosen));
        let ((diff, line, moved), patch) = if side == 0 {
            (
                (
                    chosen.added,
                    chosen.added_background,
                    adjusted.added_background,
                ),
                roles.string,
            )
        } else {
            (
                (
                    chosen.removed,
                    chosen.removed_background,
                    adjusted.removed_background,
                ),
                roles.removed,
            )
        };
        let word = words[side];
        assert!(
            lightness_beyond(off[side], line, chosen) < 0.,
            "{case}: #{:06x} drawn off on its line's canvas side",
            off[side]
        );
        assert_eq!(moved, line, "{case}: the line keeps its tint");
        assert_eq!(
            word,
            EditorHighlights::crossed_word_tint(roles, [diff, patch], (line, off[side]), chosen),
            "{case}: the word steps across"
        );
        let beyond = lightness_beyond(word, line, chosen);
        let apart = oklab_distance(oklab(word), oklab(line));
        let colors: Vec<u32> = roles.word_colors().into_iter().chain([patch]).collect();
        assert!(
            beyond > 0. && apart >= HIGHLIGHT_DISTANCE,
            "{case}: #{word:06x} {beyond:+.4} beyond its line #{line:06x}, {apart:.4} from it"
        );
        assert_eq!(
            beyond >= HIGHLIGHT_LIGHTNESS_GAP,
            at_gap,
            "{case}: #{word:06x} {beyond:+.4} beyond its line"
        );
        assert_eq!(
            the_gap_cannot_read(&colors, diff, line, chosen),
            !at_gap,
            "{case}: whether the text rule keeps the word short of the gap"
        );
        for &color in &colors {
            assert!(
                contrast(color, word) >= LABEL_RULE,
                "{case}: #{color:06x} {:.3}:1 on #{word:06x}",
                contrast(color, word)
            );
        }
        if let Some(pinned) = pinned {
            assert_eq!(word, pinned, "{case}: #{word:06x}, not #{pinned:06x}");
        }
        // The furthest step: the next one in the word's row reads some
        // color below the rule the word keeps, or the row ends.
        let seed = EditorHighlights::word_seed(diff, line);
        let rows = [seed, mix(seed, diff, 128), diff].map(|hue| beyond_line(hue, line, chosen));
        let (row, at) = rows
            .iter()
            .find_map(|row| {
                row.iter()
                    .position(|&step| step == word)
                    .map(|at| (row, at))
            })
            .unwrap_or_else(|| panic!("{case}: #{word:06x} is a step beyond its line"));
        let keeps = |step: u32, rule: f64| {
            colors
                .iter()
                .all(|&color| contrast(color, step) >= drawn_target(color, &[line], rule))
        };
        let rule = [MORE_CONTRAST_TEXT, LABEL_RULE + LABEL_MARGIN, LABEL_RULE]
            .into_iter()
            .find(|&rule| keeps(word, rule))
            .unwrap_or_else(|| panic!("{case}: #{word:06x} reads the text rule"));
        if let Some(&next) = row.get(at + 1) {
            assert!(
                !keeps(next, rule),
                "{case}: the next step #{next:06x} beyond #{word:06x} also reads {rule}:1"
            );
        }
    }
}

/// Under More contrast a word drawn off on its line's canvas side keeps
/// its tint drawn off where no step beyond its line both reads the text
/// rule and stands [`HIGHLIGHT_DISTANCE`] from it: a custom theme on
/// Midnight whose added lines are pale, `#607372`, so the text on a word
/// stepped across them reads under 4.5:1 by the step that stands apart.
/// The nearest step that stood apart, `#588177`, broke the text floor.
#[test]
fn a_crossed_word_with_no_readable_step_keeps_its_tint_drawn_off() {
    let mut custom = CustomTheme::from_base(1, "Pale lines", ThemeChoice::Midnight);
    custom.palette.set(TokenKind::AddedBackground, 0x607372);
    let chosen = custom.palette;
    let (adjusted, words, off) = diff_tints(chosen);
    let roles = adjusted.syntax_roles(Some(chosen));
    let (diff, line) = (chosen.added, chosen.added_background);
    assert!(
        lightness_beyond(off[0], line, chosen) < 0.,
        "#{:06x} drawn off on its line's canvas side",
        off[0]
    );
    assert_eq!(adjusted.added_background, line, "the line keeps its tint");
    let colors: Vec<u32> = roles
        .word_colors()
        .into_iter()
        .chain([roles.string])
        .collect();
    // Along each row the word steps in, every step that stands apart
    // reads some color under the text rule.
    let seed = EditorHighlights::word_seed(diff, line);
    for hue in [seed, mix(seed, diff, 128), diff] {
        for step in beyond_line(hue, line, chosen) {
            assert!(
                oklab_distance(oklab(step), oklab(line)) < HIGHLIGHT_DISTANCE
                    || colors
                        .iter()
                        .any(|&color| contrast(color, step) < LABEL_RULE),
                "#{step:06x} reads and stands apart"
            );
        }
    }
    assert_eq!(words[0], off[0], "#{:06x}, not #{:06x}", words[0], off[0]);
    for &color in &colors {
        assert!(
            contrast(color, words[0]) >= LABEL_RULE,
            "#{color:06x} {:.3}:1 on #{:06x}",
            contrast(color, words[0]),
            words[0]
        );
    }
}

/// Under More contrast a word that cannot lift moves its line, in
/// built-in palettes. Tokyo Night's added word lies between its line and
/// the text, 0.013 from the line, so no step of a lift keeps the text;
/// the line, 0.089 from the canvas, moves away from the word to the first
/// step at the gap, its hue kept, and the word keeps the tint drawn off.
/// The text reads better on it, and where a line tint binds secondary
/// text and line numbers, as with a darker selected row, they are fitted
/// on the moved line. Solarized Light's added line, 0.047 from its
/// canvas, has less room than its word needs: it stops at the last step
/// that stands 0.04 from the canvas, short of the gap.
#[test]
fn a_word_that_cannot_lift_moves_its_line_to_the_gap() {
    let gap = |word: u32, line: u32| lightness_gap(word, line).abs();
    let chosen = ThemeChoice::TokyoNight.palette();
    let (adjusted, words, off) = diff_tints(chosen);
    let (line, moved, word) = (chosen.added_background, adjusted.added_background, words[0]);
    assert_eq!(
        word, off[0],
        "Tokyo Night's added word keeps the tint drawn off"
    );
    assert!(gap(word, line) < 0.02);
    assert!(
        gap(word, moved) >= WORD_LIGHTNESS_GAP,
        "#{moved:06x} {:.4} from #{word:06x}",
        gap(word, moved)
    );
    let moves = line_moves(line, word, (off[0], line), chosen.canvas);
    assert_eq!(moves.last(), Some(&moved));
    assert!(
        moves[..moves.len() - 1]
            .iter()
            .all(|&step| gap(word, step) < WORD_LIGHTNESS_GAP)
    );
    assert!(from_canvas(chosen, moved) >= WORD_LIGHTNESS_GAP);
    assert!(hue_difference(moved, line) < 0.05);
    assert!(contrast(chosen.text, moved) > contrast(chosen.text, line));

    // Where a line tint binds secondary text and line numbers, they are
    // fitted on the moved line: with a selected row #1a2030 darker than
    // Tokyo Night's line tints, each is the least step toward `text` that
    // reads on the surfaces as adjusted, nearer its own token than the
    // step the lines drawn off need.
    let chosen = Palette {
        selected: 0x1a2030,
        ..ThemeChoice::TokyoNight.palette()
    };
    let adjusted = chosen.with_more_contrast();
    let lines = |palette: Palette| (palette.added_background, palette.removed_background);
    assert_ne!(lines(adjusted), lines(chosen), "the line tints move");
    let on_lines_drawn_off = chosen.with_raised_floors();
    let surfaces = more_contrast_text_surfaces(adjusted);
    let reads = |color: u32| {
        surfaces.iter().all(|&(_, surface, momentary)| {
            contrast(color, surface) >= more_contrast_text_floor(adjusted, surface, momentary)
        })
    };
    for (token, own, fitted, drawn_off) in [
        (
            "muted",
            chosen.muted,
            adjusted.muted,
            on_lines_drawn_off.muted,
        ),
        (
            "line_number",
            chosen.line_number,
            adjusted.line_number,
            on_lines_drawn_off.line_number,
        ),
    ] {
        assert_ne!(fitted, drawn_off, "{token} fitted on the lines drawn off");
        let step = (0..=256)
            .find(|&step| mix(own, chosen.text, step) == fitted)
            .expect("a step toward text");
        assert!(
            reads(fitted) && (step == 0 || !reads(mix(own, chosen.text, step - 1))),
            "{token} #{fitted:06x} is the least step that reads"
        );
    }

    let chosen = ThemeChoice::SolarizedLight.palette();
    let (adjusted, words, off) = diff_tints(chosen);
    let (line, moved, word) = (chosen.added_background, adjusted.added_background, words[0]);
    assert_ne!(moved, line, "Solarized Light's added line moves");
    assert!(gap(word, moved) < WORD_LIGHTNESS_GAP && gap(word, moved) > gap(word, line));
    let moves = line_moves(line, word, (off[0], line), chosen.canvas);
    assert_eq!(moves.last(), Some(&moved));
    let reach = from_canvas(chosen, moved);
    assert!(
        (WORD_LIGHTNESS_GAP..WORD_LIGHTNESS_GAP + 0.004).contains(&reach),
        "#{moved:06x} {reach:.4} from the canvas"
    );
}

/// Under More contrast a moved line stops at each of its bounds, in
/// built-in palettes and one built from Tokyo Night. Nord's added line
/// stands 0.025 from its canvas, within 0.04, so though its word stands
/// 0.032 from it between the line and the text, the line keeps its tint.
/// One Dark's added word, drawn off on the canvas side of its line,
/// steps across it, and its line keeps its tint: moved away from the
/// word, the line would step toward the text, and its first step reads
/// `text` worse than the line drawn off. Tokyo Night with a selected row #002c28 near its
/// added line's path stops the line where the next step would stand
/// nearer the selection than 0.04, short of the gap it reaches with its
/// own selection.
#[test]
fn a_moved_line_stops_at_its_bounds() {
    let gap = |word: u32, line: u32| lightness_gap(word, line).abs();
    let chosen = ThemeChoice::Nord.palette();
    let (adjusted, words, off) = diff_tints(chosen);
    let line = chosen.added_background;
    assert!(from_canvas(chosen, line) < WORD_LIGHTNESS_GAP);
    assert!(gap(words[0], line) < WORD_LIGHTNESS_GAP);
    assert!(line_moves(line, words[0], (off[0], line), chosen.canvas).is_empty());
    assert_eq!(adjusted.added_background, line, "Nord's added line");

    let chosen = ThemeChoice::OneDark.palette();
    let (adjusted, words, off) = diff_tints(chosen);
    let line = chosen.added_background;
    assert!(lightness_beyond(off[0], line, chosen) < 0. && gap(off[0], line) < WORD_LIGHTNESS_GAP);
    let moves = line_moves(line, off[0], (off[0], line), chosen.canvas);
    assert!(contrast(chosen.text, moves[0]) < contrast(chosen.text, line));
    assert_eq!(adjusted.added_background, line, "One Dark's added line");
    assert!(
        stands_beyond(words[0], line, chosen),
        "One Dark's added word steps across"
    );

    let tokyo = ThemeChoice::TokyoNight.palette();
    let chosen = Palette {
        selected: 0x002c28,
        ..tokyo
    };
    let highlights = EditorHighlights::fit(chosen, chosen.syntax_roles(None), None);
    assert_eq!(highlights.selection, 0x002c28);
    let (adjusted, words, off) = diff_tints(chosen);
    let (line, moved) = (chosen.added_background, adjusted.added_background);
    let apart = |color: u32| oklab_distance(oklab(highlights.selection), oklab(color));
    assert!(apart(line) >= HIGHLIGHT_DISTANCE && apart(moved) >= HIGHLIGHT_DISTANCE);
    assert!(gap(words[0], moved) < WORD_LIGHTNESS_GAP);
    assert_ne!(moved, line, "the line moves up to the selection's bound");
    assert_eq!(tokyo.with_more_contrast().added_background, 0x1f2f25);
    let moves = line_moves(line, words[0], (off[0], line), chosen.canvas);
    let next = moves[moves.iter().position(|&step| step == moved).unwrap() + 1];
    assert!(
        apart(next) < HIGHLIGHT_DISTANCE,
        "#{next:06x} {:.4}",
        apart(next)
    );
}

/// A line's move stops at the step before the first that fails a bound,
/// never passing it for a later step that holds every bound, in a palette
/// built from Midnight (the faint custom palette). Its removed word reads
/// lighter than its line in normal vision and darker in a protanopia
/// model, so the first steps of the line's move narrow that gap, then
/// cross it, and the steps past the crossing widen it again and hold
/// every bound, the last at the word's 0.04. The line keeps the tint drawn
/// off; kept to the steps that hold, it moved to #32202b.
#[test]
fn a_moved_line_never_passes_a_step_that_fails_a_bound() {
    let chosen = faint_custom_palette();
    let (adjusted, _, off) = diff_tints(chosen);
    let words = words_lines_move_for(chosen);
    let highlights = EditorHighlights::fit(chosen, chosen.syntax_roles(None), None);
    let (line, word) = (chosen.removed_background, words[1]);
    let holds = |step: u32| {
        let candidate = Palette {
            added_background: adjusted.added_background,
            removed_background: step,
            ..chosen
        }
        .with_raised_floors();
        highlights.stay_apart_from(step, line)
            && keeps_colour_blind_gaps(word, step, line)
            && chosen.diff_reads_as_off(chosen.syntax_roles(None), candidate, off, words)
    };
    let moves = line_moves(line, word, (off[1], line), chosen.canvas);
    assert!(
        !keeps_colour_blind_gaps(word, moves[0], line),
        "the first step narrows the word's gap in protanopia"
    );
    let past = moves.iter().copied().rev().find(|&step| holds(step));
    assert!(
        past.is_some_and(|step| lightness_gap(word, step).abs() >= WORD_LIGHTNESS_GAP),
        "a later step holds every bound at the gap: {past:x?}"
    );
    assert_eq!(
        adjusted.removed_background, line,
        "the removed line keeps its tint"
    );
}

/// A raised highlight keeps at least the luminance contrast with its line
/// that the one drawn with More contrast off has, on the same side, or
/// that one stays: the bound the selection and Find's matches are raised
/// under ([`EditorHighlights::raised`]). The fixtures are word-tint
/// inputs, where the bound was found, run through that raise directly;
/// no product path raises a word tint now, since word tints are lifted
/// ([`EditorHighlights::lifted_word_tint`]). Midnight's removed word and
/// Porcelain's added word, held only to their distance from their line
/// and their side of it in OKLab lightness, were raised through
/// saturation alone to their line's luminance (#5d0016 and #aefed9, 1.00
/// to 1.01:1, where the tints drawn off stand 1.37:1 and 1.22:1 from
/// their lines), and in a
/// protanopia and deuteranopia model the removed word turned darker than
/// its line and the added word fell from 0.067 to 0.018 from it; held to
/// the luminance too, none is raised. A highlight on the far side of its
/// line from the text on it, darker than a dark line under light text
/// here, is still raised: away from the line, so its text reaches 7:1
/// and the highlight stands further from the line in luminance than off.
#[test]
fn a_raised_highlight_keeps_its_off_luminance_contrast_with_its_line() {
    for (case, theme, removed) in [
        ("Midnight removed-word inputs", ThemeChoice::Midnight, true),
        ("Porcelain added-word inputs", ThemeChoice::Porcelain, false),
    ] {
        let chosen = theme.palette();
        let palette = chosen.with_more_contrast();
        let roles = palette.syntax_roles(Some(chosen));
        let (diff, line) = if removed {
            (palette.removed, palette.removed_background)
        } else {
            (palette.added, palette.added_background)
        };
        let own_roles = chosen.syntax_roles(None);
        let off = EditorHighlights::word_tint(chosen, own_roles, diff, line);
        let (lines, on_line) = (EditorHighlights::lines(palette), [line]);
        let drawn: Vec<_> = EditorHighlights::word_colors(roles, &lines, diff, &on_line).collect();
        let own: Vec<_> = own_roles.word_colors().into_iter().chain([diff]).collect();
        let seed = EditorHighlights::word_seed(diff, line);
        let away = EditorHighlights::away(palette);
        let fit = || EditorHighlights::raised(&drawn, &own, off, &on_line);
        assert_eq!(fit().raise(seed, away, diff), None, "{case}: none raised");
        let mut by_distance = fit();
        for apart in &mut by_distance.lines {
            apart.beyond = None;
        }
        let saturated = by_distance
            .raise(seed, away, diff)
            .expect("held only to distance and side, a tint is raised");
        assert!(
            contrast(saturated, line) < contrast(off, line),
            "{case}: held only to distance and side, #{saturated:06x} {:.3}:1 on its line, \
             off #{off:06x} {:.3}:1",
            contrast(saturated, line),
            contrast(off, line)
        );
    }

    // Darker than its dark line under light text, a highlight is raised
    // away from the line until the text reads 7:1 (6.29:1 on the one
    // drawn off).
    let (line, off, text) = (0x30343a, 0x202328, 0xa0a4aa);
    assert!(contrast(text, off) < MORE_CONTRAST_TEXT);
    let raised = HighlightFit::targeted(&[(text, MORE_CONTRAST_TEXT)], &[line])
        .at_least_as_apart_as(off)
        .raise(off, 0x000000, off)
        .expect("raised away from its line");
    assert!(
        contrast(text, raised) >= MORE_CONTRAST_TEXT
            && custom::luminance(raised) < custom::luminance(off)
            && fainter_on_its_line(raised, off, line).is_none(),
        "#{raised:06x}: text {:.3}:1, {:.3}:1 on its line, off {:.3}:1",
        contrast(text, raised),
        contrast(raised, line),
        contrast(off, line)
    );
}

/// [`EditorHighlights::current_beside`] in literal greys, each way it
/// can place the current match beside the selection, always at least
/// [`HIGHLIGHT_DISTANCE`] and [`HIGHLIGHT_LIGHTNESS_GAP`] in lightness
/// from it: raised to stand from the raised selection as far as the off
/// current match stands from the off selection, or at least that floor
/// where the two drawn off stand nearer; where none is raised, the off
/// current match beside the raised selection, or beside the off one,
/// where it stands so; or else fitted by the text rule beside the raised
/// selection, or beside the off one. Raised only as far as off where
/// that was less, the current match stood 0.003 from the selection in
/// Sandstone; kept beside a selection it stood 0.044 from by chroma
/// alone, 0.0001 in lightness in One Dark. Held to its distance from the
/// raised selection alone, the current match settled beside the other
/// matches (Omarchy alacritty-dark 0.040 to 0.024), and the raised
/// selection beside the off current match (Omarchy ristretto 0.090 to
/// 0.039).
#[test]
fn the_current_match_stands_from_a_raised_selection_as_far_as_off() {
    let apart = |a: u32, b: u32| oklab_distance(oklab(a), oklab(b));
    let (off_selection, near, far) = (0x404040, 0x434343, 0x707070);
    assert!(apart(off_selection, near) < HIGHLIGHT_DISTANCE);
    assert!(apart(off_selection, far) >= HIGHLIGHT_DISTANCE);
    // The selection's lightness at 0.05 of chroma: apart by chroma alone.
    let [lightness, ..] = oklab(off_selection);
    let tinted = from_linear(linear_in_gamut([lightness, 0.05, 0.]));
    assert!(apart(off_selection, tinted) >= HIGHLIGHT_DISTANCE);
    assert!((oklab(tinted)[0] - lightness).abs() < HIGHLIGHT_LIGHTNESS_GAP);
    let asked = std::cell::RefCell::new(Vec::new());
    let raised = |current: u32, from: f64| {
        let asked = &asked;
        move |distance: f64| {
            asked.borrow_mut().push(distance);
            (distance <= from).then_some(current)
        }
    };
    let none = || raised(0x000000, f64::NEG_INFINITY);
    let fits = std::cell::RefCell::new(Vec::new());
    // A fitted candidate beside `beside` (or any selection) that stands
    // at most `reach` from it.
    let fitted = |current: Option<u32>, beside: Option<u32>, reach: f64| {
        let fits = &fits;
        move |selection: u32, distance: f64| {
            fits.borrow_mut().push((selection, distance));
            current.filter(|_| beside.is_none_or(|beside| beside == selection) && distance <= reach)
        }
    };
    let never = || fitted(None, None, 0.);

    // Raised as far from the raised selection as off.
    let off_apart = apart(off_selection, far);
    assert_eq!(
        EditorHighlights::current_beside(
            (off_selection, far),
            0x383838,
            raised(0xaa2222, 1.),
            never()
        ),
        (0x383838, 0xaa2222),
        "raised"
    );
    assert_eq!(asked.take(), [off_apart], "asked as far as off");
    assert!(fits.take().is_empty(), "none fitted");

    // Drawn off nearer than the floor: raised to the floor, never only
    // as far as off.
    assert_eq!(
        EditorHighlights::current_beside(
            (off_selection, near),
            0x383838,
            raised(0x505050, 1.),
            never()
        ),
        (0x383838, 0x505050),
        "raised to the floor"
    );
    assert_eq!(asked.take(), [HIGHLIGHT_DISTANCE], "asked at the floor");

    // None raised: the off current match beside the raised selection
    // that stands from it as far as off, with the lightness gap.
    assert!(apart(far, 0x202020) >= off_apart);
    assert_eq!(
        EditorHighlights::current_beside((off_selection, far), 0x202020, none(), never()),
        (0x202020, far),
        "the off current match beside the raised selection"
    );
    assert_eq!(asked.take(), [off_apart], "asked once, as far as off");
    // Nearer the raised selection than off: both drawn off.
    assert!(apart(far, 0x606060) < off_apart);
    assert_eq!(
        EditorHighlights::current_beside((off_selection, far), 0x606060, none(), never()),
        (off_selection, far),
        "both drawn off"
    );
    assert!(
        fits.take().is_empty(),
        "none fitted while the off pair stands apart"
    );
    asked.take();

    // Drawn off nearer than the floor, or apart by chroma alone: fitted
    // beside the raised selection, else beside the off one, as far as
    // the pair drawn off stand apart, at least the floor.
    for off_current in [near, tinted] {
        let as_off = apart(off_selection, off_current).max(HIGHLIGHT_DISTANCE);
        assert_eq!(
            EditorHighlights::current_beside(
                (off_selection, off_current),
                0x3c3c3c,
                none(),
                fitted(Some(0x606060), None, 1.)
            ),
            (0x3c3c3c, 0x606060),
            "fitted beside the raised selection, #{off_current:06x} drawn off"
        );
        assert_eq!(
            fits.take(),
            [(0x3c3c3c, as_off)],
            "fitted beside the raised selection, as far as off"
        );
        assert_eq!(
            EditorHighlights::current_beside(
                (off_selection, off_current),
                0x3c3c3c,
                none(),
                fitted(Some(0x606060), Some(off_selection), 1.)
            ),
            (off_selection, 0x606060),
            "fitted beside the off selection, #{off_current:06x} drawn off"
        );
        assert_eq!(
            fits.take(),
            [(0x3c3c3c, as_off), (off_selection, as_off)],
            "then beside the off one"
        );
    }
    // Where no fitted candidate stands as far as off, beside either
    // selection, the floor: beside the raised selection first.
    let as_off = apart(off_selection, tinted);
    assert!(as_off > HIGHLIGHT_DISTANCE);
    assert_eq!(
        EditorHighlights::current_beside(
            (off_selection, tinted),
            0x3c3c3c,
            none(),
            fitted(Some(0x606060), None, HIGHLIGHT_DISTANCE)
        ),
        (0x3c3c3c, 0x606060),
        "fitted at the floor"
    );
    assert_eq!(
        fits.take(),
        [
            (0x3c3c3c, as_off),
            (off_selection, as_off),
            (0x3c3c3c, HIGHLIGHT_DISTANCE)
        ],
        "as far as off beside both selections, then the floor"
    );
    asked.take();
}

/// Fitted by the text rule, Find's current match keeps the distance the
/// pair drawn off stand apart wherever a fitted candidate can: here a
/// grey selection and a current match drawn off 0.10 apart through
/// chroma, under [`HIGHLIGHT_LIGHTNESS_GAP`] apart in lightness, so the
/// current match drawn off cannot stay, and none is raised. Fitted only
/// [`HIGHLIGHT_DISTANCE`] from the selection, as before, the current
/// match stood nearer it than the pair drawn off (One Dark's `#104266`,
/// 0.041 from its selection, where the pair drawn off stand 0.044 apart
/// through chroma).
#[test]
fn a_fitted_current_match_keeps_its_off_distance_where_it_can() {
    let apart = |a: u32, b: u32| oklab_distance(oklab(a), oklab(b));
    let (canvas, text, accent) = (0x202020, 0xe0e0e0, 0xc8553d);
    let off_selection = 0x404040;
    let [lightness, ..] = oklab(off_selection);
    let off_current = from_linear(linear_in_gamut([lightness, 0.102, 0.]));
    let off_apart = apart(off_selection, off_current);
    assert!(off_apart >= 0.1, "{off_apart:.4}");
    assert!((oklab(off_current)[0] - lightness).abs() < HIGHLIGHT_LIGHTNESS_GAP);
    // As [`EditorHighlights::fit_raised`] fits one: the text rule, toward
    // black and then toward white.
    let seed = mix(off_selection, accent, 64);
    let fitted = |selection: u32, distance: f64| {
        let fit =
            HighlightFit::new(&[(text, &[canvas][..])], &[canvas]).apart_from(selection, distance);
        fit.search(seed, 0x000000, accent)
            .or_else(|| fit.search(seed, 0xffffff, accent))
    };
    let floor = fitted(off_selection, HIGHLIGHT_DISTANCE).expect("one at the floor");
    assert!(
        apart(floor, off_selection) < off_apart,
        "at the floor #{floor:06x} {:.4} from the selection",
        apart(floor, off_selection)
    );
    let (selection, current) = EditorHighlights::current_beside(
        (off_selection, off_current),
        off_selection,
        |_| None,
        fitted,
    );
    assert_eq!(selection, off_selection);
    assert!(
        apart(current, selection) >= off_apart
            && (oklab(current)[0] - oklab(selection)[0]).abs() >= HIGHLIGHT_LIGHTNESS_GAP,
        "#{current:06x} {:.4} from the selection, off {off_apart:.4}",
        apart(current, selection)
    );
    assert!(contrast(text, current) >= LABEL_RULE);
}

/// Find's current match keeps the distance it stands from the other
/// matches with More contrast off, in two synthetic cases built from
/// built-in palettes, so a run without the Omarchy palettes covers it:
/// Midnight and Graphite with a grey selected row and a warm accent, in
/// which the current match drawn off stands at least
/// [`HIGHLIGHT_DISTANCE`] from the other matches. The selection keeps the
/// one drawn off, and the current match stands as far as off from the
/// selection and from the other matches, which take a colour of their
/// own, and [`HIGHLIGHT_LIGHTNESS_GAP`] from the selection in lightness
/// (the pair drawn off stand 0.05 apart, under 0.02 in lightness, so the
/// current match moves). Held only to its distance from its lines, the selection alone
/// was once raised toward the current match drawn off here (Midnight's
/// #44251e to #551205, 0.012 from the current match #591509, where the
/// selection drawn off stands 0.053 from it); held also to its luminance
/// contrast with each line, it is not. Each way the current match is
/// placed beside a raised selection is read in
/// `the_current_match_stands_from_a_raised_selection_as_far_as_off`.
#[test]
fn the_current_match_keeps_its_off_distance_from_the_other_matches() {
    for (case, base, selected, accent) in [
        ("Midnight", ThemeChoice::Midnight, 0x383838, 0xc8553d),
        ("Graphite", ThemeChoice::Graphite, 0x343335, 0xb5651d),
    ] {
        let mut custom = CustomTheme::from_base(1, case, base);
        custom.palette.set(TokenKind::Selected, selected);
        custom.palette.set(TokenKind::Accent, accent);
        let chosen = custom.palette;
        let (palette, raised) = chosen.more_contrast_fit();
        let roles = palette.syntax_roles(Some(chosen));
        let off = EditorHighlights::fit(chosen, chosen.syntax_roles(None), None);
        let on = EditorHighlights::fit(palette, roles, Some(raised));
        let apart = |current: u32, others: u32| oklab_distance(oklab(current), oklab(others));
        let off_apart = apart(off.find_current, off.selection);
        assert!(
            off_apart >= HIGHLIGHT_DISTANCE,
            "{case}: the current match drawn off stands {off_apart:.3} from the others"
        );

        assert_eq!(
            on.selection, off.selection,
            "{case}: the selection drawn off"
        );
        for (others, color) in [
            ("selection", on.selection),
            ("other matches", on.find_match),
        ] {
            assert!(
                apart(on.find_current, color) >= off_apart,
                "{case}: the current match #{:06x} {:.3} from the {others} #{color:06x}, off \
                 {off_apart:.3}",
                on.find_current,
                apart(on.find_current, color),
            );
        }
        let lightness = (oklab(on.find_current)[0] - oklab(on.selection)[0]).abs();
        assert!(
            lightness >= HIGHLIGHT_LIGHTNESS_GAP,
            "{case}: the current match #{:06x} {lightness:.4} from the selection in lightness",
            on.find_current
        );
    }
}

/// With More contrast on, no color reads worse on the text selection than
/// it does with More contrast off, nor on Find's current match where it
/// keeps or raises the one drawn off, in every palette (Find's other
/// matches are fitted by the text rule, never raised):
/// `text`, every syntax role and the patch's `added`,
/// `removed` and `hunk`, the colors those highlights are fitted under, each
/// read on the highlight drawn on at least the contrast it reads on the
/// one drawn off, and where the highlight moved, 7:1 or its own contrast
/// on the lines it is drawn on where that is lower; each highlight stands
/// from every editor line, in OKLab, at least as far as the one drawn off
/// and on the same side of it in lightness, so in a dark palette none
/// turns darker than its darkest line where the one drawn off is lighter;
/// and wherever the current match drawn off stands
/// [`HIGHLIGHT_DISTANCE`] from the other matches, the current match
/// stands from them at least as far. Where the current match or the
/// other matches can do neither and stand apart as the rules of
/// 2026-11-05 afternoon ask (the current match [`HIGHLIGHT_DISTANCE`]
/// and [`HIGHLIGHT_LIGHTNESS_GAP`] from the selection, the other matches
/// in a colour of their own), each is fitted by the text rule instead:
/// every color on it reads 4.5:1 and it stands [`HIGHLIGHT_DISTANCE`]
/// from every editor line. Fitted only to the text rule against
/// the raised syntax, they settled lighter and nearer their seeds, and
/// 213 colors read worse on them across the palettes (Midnight's strings
/// on the current match 6.18:1 where they read 6.63:1); held to the
/// distance alone, the selection turned near black in seven dark
/// palettes (Midnight's #0b3e3e to #000001), and with the current
/// match's distance taken from the raised selection, the current match
/// settled beside the other matches (Omarchy alacritty-dark 0.040 to
/// 0.024, ristretto 0.090 to 0.039).
#[gpui::test]
fn more_contrast_never_reads_worse_on_the_selection_or_find(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    const DRAWN: [&str; 12] = [
        "keyword",
        "string",
        "constant",
        "type",
        "function",
        "markup",
        "comment",
        "removed",
        "text",
        "added",
        "removed lines",
        "hunk",
    ];
    let mut failures = Vec::new();
    let (mut raised, mut kept, mut separated) = (0, 0, 0);
    let (mut current_as_off, mut current_fitted, mut matches_fitted) = (0, 0, 0);
    for (name, chosen) in more_contrast_palettes() {
        let [off, on] = [false, true].map(|more| {
            cx.update(|cx| {
                set_more_contrast(more, cx);
                chosen.apply(chosen.is_light(), None, cx);
                let applied = palette(cx);
                let lines = EditorHighlights::lines(applied);
                let drawn: Vec<(u32, Vec<u32>)> = EditorHighlights::patch_colors(
                    applied,
                    applied.syntax_roles(more.then_some(chosen)),
                    &lines,
                )
                .into_iter()
                .map(|(color, on)| (color, on.to_vec()))
                .collect();
                (editor_highlights(cx), drawn, lines)
            })
        });
        let ((off_tints, was, _), (on_tints, is, lines)) = (off, on);
        if on_tints.selection == off_tints.selection {
            kept += 1;
        } else {
            raised += 1;
        }
        let darkest = lines
            .iter()
            .map(|&line| oklab(line)[0])
            .fold(f64::INFINITY, f64::min);
        for (highlight, off_tint, on_tint) in [
            ("selection", off_tints.selection, on_tints.selection),
            ("Find match", off_tints.find_match, on_tints.find_match),
            (
                "current Find match",
                off_tints.find_current,
                on_tints.find_current,
            ),
        ] {
            // How the highlight reads worse or stands nearer than off, if
            // it does.
            let mut worse = Vec::new();
            for ((color, (was, _)), (is, on)) in DRAWN.iter().zip(&was).zip(&is) {
                let (was, is) = (*was, *is);
                let (off_ratio, on_ratio) = (contrast(was, off_tint), contrast(is, on_tint));
                if on_ratio < off_ratio {
                    worse.push(format!(
                        "{name} {color} #{is:06x} on the {highlight} #{on_tint:06x} \
                         {on_ratio:.3}:1, off #{was:06x} on #{off_tint:06x} {off_ratio:.3}:1"
                    ));
                }
                let floor = on
                    .iter()
                    .map(|&beneath| contrast(is, beneath))
                    .fold(MORE_CONTRAST_TEXT, f64::min)
                    .max(LABEL_RULE);
                if on_tint != off_tint && on_ratio < floor {
                    worse.push(format!(
                        "{name} {color} on the moved {highlight} #{on_tint:06x} \
                         {on_ratio:.3}:1, needs {floor:.3}"
                    ));
                }
            }
            for line in lines {
                let apart = |tint: u32| oklab_distance(oklab(tint), oklab(line));
                if apart(on_tint) < apart(off_tint) {
                    worse.push(format!(
                        "{name} {highlight} #{on_tint:06x} {:.3} from the line #{line:06x}, \
                         off #{off_tint:06x} {:.3}",
                        apart(on_tint),
                        apart(off_tint)
                    ));
                }
                let side = |tint: u32| oklab(tint)[0].total_cmp(&oklab(line)[0]);
                if side(on_tint) != side(off_tint) {
                    worse.push(format!(
                        "{name} {highlight} #{on_tint:06x} {:?} the line #{line:06x} in \
                         lightness, off #{off_tint:06x} {:?}",
                        side(on_tint),
                        side(off_tint)
                    ));
                }
                if let Some(failure) = fainter_on_its_line(on_tint, off_tint, line) {
                    worse.push(format!("{name} {highlight} {failure}"));
                }
            }
            if !chosen.is_light() && oklab(off_tint)[0] > darkest && oklab(on_tint)[0] <= darkest {
                worse.push(format!(
                    "{name} {highlight} #{on_tint:06x} darker than the darkest line, off \
                     #{off_tint:06x} lighter"
                ));
            }
            if worse.is_empty() {
                current_as_off += usize::from(highlight == "current Find match");
                continue;
            }
            // The selection is always raised or drawn off; the current
            // match and the other matches may be fitted by the text rule.
            // Under More contrast the patch draws its text in the fitted
            // roles ([`PatchText`]), so the diff colors are not text there.
            let fitted = highlight != "selection"
                && is
                    .iter()
                    .zip(DRAWN)
                    .filter(|(_, color)| !matches!(*color, "added" | "removed lines" | "hunk"))
                    .all(|(&(color, _), _)| contrast(color, on_tint) >= LABEL_RULE)
                && lines
                    .iter()
                    .all(|&line| oklab_distance(oklab(on_tint), oklab(line)) >= HIGHLIGHT_DISTANCE);
            if fitted {
                match highlight {
                    "current Find match" => current_fitted += 1,
                    _ => matches_fitted += 1,
                }
            } else {
                failures.extend(worse);
            }
        }
        let beside = |tints: EditorHighlights| {
            oklab_distance(oklab(tints.find_current), oklab(tints.find_match))
        };
        if beside(off_tints) >= HIGHLIGHT_DISTANCE {
            separated += 1;
            if beside(on_tints) < beside(off_tints) {
                failures.push(format!(
                    "{name} current Find match #{:06x} {:.3} from the other matches \
                     #{:06x}, off #{:06x} {:.3} from #{:06x}",
                    on_tints.find_current,
                    beside(on_tints),
                    on_tints.find_match,
                    off_tints.find_current,
                    beside(off_tints),
                    off_tints.find_match
                ));
            }
        }
    }
    cx.update(|cx| set_more_contrast(false, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Held to their off luminance contrast with every line, the
    // selection and current match are rarely raised; each way the
    // current match is placed beside a raised selection is read in
    // `the_current_match_stands_from_a_raised_selection_as_far_as_off`.
    assert!(
        kept > 0 && separated > 0 && current_as_off > 0 && current_fitted > 0 && matches_fitted > 0,
        "selections raised {raised}, kept {kept}; current matches apart off {separated}, \
         kept or raised {current_as_off}, fitted {current_fitted}; other matches fitted \
         {matches_fitted}"
    );
}

/// The controls [`FieldControls`] draws on each surface, by kind: a text
/// field with a placeholder, enabled and disabled, and Settings' Save
/// (the kit's default Button) enabled and disabled.
const FIELD_CONTROLS: [&str; 4] = ["field", "disabled-field", "save", "disabled-save"];

/// Each of [`FIELD_CONTROLS`] in a row on each of
/// [`more_contrast_edge_surfaces`] of the applied palette, selected as
/// `kind-surface`.
struct FieldControls {
    /// Each surface's field and disabled field.
    fields: Vec<[gpui::Entity<gpui_kit::component::input::InputState>; 2]>,
}

impl FieldControls {
    fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        use gpui_kit::AppContext as _;
        use gpui_kit::component::input::InputState;

        let fields = more_contrast_edge_surfaces(palette(cx))
            .map(|_| [(); 2].map(|_| cx.new(|cx| InputState::new(window, cx).placeholder("Title"))))
            .into();
        Self { fields }
    }
}

impl gpui::Render for FieldControls {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui_kit::component::Disableable as _;
        use gpui_kit::component::button::Button;
        use gpui_kit::component::input::Input;
        use gpui_kit::{InteractiveElement as _, ParentElement as _, div};

        let surfaces = more_contrast_edge_surfaces(palette(cx));
        div()
            .flex()
            .flex_col()
            .children(surfaces.into_iter().zip(self.fields.clone()).map(
                |((surface, color), [field, disabled_field])| {
                    let id = move |kind: &'static str| resting_selector(kind, surface);
                    let selector = move |kind: &'static str| move || id(kind).into();
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .bg(rgb(color))
                        .child(
                            div()
                                .w(px(200.))
                                .debug_selector(selector("field"))
                                .child(Input::new(&field)),
                        )
                        .child(
                            div()
                                .w(px(200.))
                                .debug_selector(selector("disabled-field"))
                                .child(Input::new(&disabled_field).disabled(true)),
                        )
                        .child(
                            // Mirrors `settings.rs`'s Save chain: the kit's default Button.
                            Button::new(id("save"))
                                .label("Save")
                                .debug_selector(selector("save")),
                        )
                        .child(
                            Button::new(id("disabled-save"))
                                .label("Save")
                                .disabled(true)
                                .debug_selector(selector("disabled-save")),
                        )
                },
            ))
    }
}

/// A fill is not an edge. With More contrast on, in every palette, a text
/// field and Settings' Save, each enabled and disabled, paint on the
/// canvas, panel, subtle and raised surfaces the fills they paint with it
/// off, while the enabled field's outline is the adjusted `border` and
/// reads 3:1 or more against each surface, and the disabled field's is
/// the disabled edge, as a disabled Button's, and reads below it on each
/// surface; off, both outlines are `border`, as the kit draws them. A
/// disabled field once drew the enabled field's 3:1 outline beside a
/// disabled Button's 1.66:1 edge (Midnight #606976 against #313d4e),
/// marked only by its dim text. The kit derived a
/// dark field's fill from `input`, the outline, at 30%, so the fill
/// lifted with the outline (Midnight #1a2330 to #2a323d), the disabled
/// Save's fill, half of it, with it, and the placeholder on the field
/// read worse (Omarchy tokyo-night 5.99:1 to 5.32:1 on the subtle
/// surface); a disabled field's fill, `input` at 80% in either mode,
/// lifted in every palette. The placeholder, drawn in `muted_foreground`,
/// now reads on the field at least as it does off on every surface
/// (tokyo-night's 6.63:1 there).
#[gpui::test]
fn more_contrast_keeps_the_field_and_save_fills_in_every_palette(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(FieldControls::new);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(gpui::size(px(800.), px(400.)));
    let mut failures = Vec::new();
    let mut tokyo_night = Vec::new();
    for (name, chosen) in test_palettes() {
        // Each control on each surface: the surface, its fills and the
        // 1 px edges it paints, and the placeholder's color.
        let [off, on] = [false, true].map(|more| {
            cx.update(|window, cx| {
                set_more_contrast(more, cx);
                chosen.apply(chosen.is_light(), Some(window), cx);
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            let placeholder = cx.update(|_, cx| packed(Theme::global(cx).muted_foreground));
            let mut drawn = Vec::new();
            for (surface, color) in more_contrast_edge_surfaces(chosen) {
                for kind in FIELD_CONTROLS {
                    let bounds = cx
                        .debug_bounds(resting_selector(kind, surface))
                        .expect(kind);
                    let fills = painted_button(cx, bounds).0;
                    let edges = painted_edges(cx, bounds);
                    drawn.push((kind, surface, color, fills, edges));
                }
            }
            (placeholder, drawn)
        });
        let ((off_placeholder, off), (placeholder, on)) = (off, on);
        let (outline, disabled_edge) = (chosen.with_more_contrast().border, chosen.disabled_edge());
        for (before, after) in off.iter().zip(&on) {
            let (kind, surface, color, fills, edges) = after;
            let at = format!("{name} {kind} on the {surface}");
            if *fills != before.3 {
                failures.push(format!("{at}: fills {fills:?}, off {:?}", before.3));
            }
            if !kind.ends_with("field") {
                continue;
            }
            let one_px = |edges: &[PaintedEdge]| -> Vec<u32> {
                edges
                    .iter()
                    .filter(|edge| edge.one_px)
                    .map(|edge| edge.color)
                    .collect()
            };
            if !one_px(&before.4).contains(&chosen.border) {
                failures.push(format!(
                    "{at}: off, outlines {:06x?}, none #{:06x}",
                    one_px(&before.4),
                    chosen.border
                ));
            }
            let drawn = if *kind == "field" {
                outline
            } else {
                disabled_edge
            };
            let outlines = one_px(edges);
            if !outlines.contains(&drawn) || (drawn != outline && outlines.contains(&outline)) {
                failures.push(format!("{at}: outlines {outlines:06x?}, not #{drawn:06x}"));
            }
            let (ratio, enabled) = (contrast(drawn, *color), contrast(outline, *color));
            if *kind != "field" {
                if ratio >= enabled {
                    failures.push(format!(
                        "{at}: outline #{drawn:06x} {ratio:.3}:1, the enabled field's \
                         #{outline:06x} {enabled:.3}:1"
                    ));
                }
                continue;
            }
            if ratio < MORE_CONTRAST_EDGE {
                failures.push(format!("{at}: outline #{drawn:06x} {ratio:.3}:1"));
            }
            let reading = |placeholder: u32, fills: &[Background]| {
                let fill = fills.last().and_then(|fill| fill.as_solid())?;
                let beneath = composite(fill.to_rgb(), *color);
                Some(contrast(placeholder, beneath))
            };
            match (
                reading(placeholder, fills),
                reading(off_placeholder, &before.3),
            ) {
                (Some(ratio), Some(off_ratio)) => {
                    if ratio < off_ratio {
                        failures.push(format!(
                            "{at}: placeholder {ratio:.3}:1, off {off_ratio:.3}:1"
                        ));
                    }
                    if name == "Omarchy tokyo-night" {
                        tokyo_night.push((*surface, ratio, off_ratio));
                    }
                }
                readings => failures.push(format!("{at}: field fills {readings:?}")),
            }
        }
    }
    cx.update(|window, cx| {
        set_more_contrast(false, cx);
        ThemeChoice::Midnight.apply(Some(window), cx);
    });
    // Omarchy tokyo-night (on Linux): its composer's field rests on the
    // subtle surface, where the placeholder read 5.99:1 off and 5.32:1 on
    // the lifted fill.
    if let Some(&(_, ratio, off_ratio)) = tokyo_night
        .iter()
        .find(|(surface, ..)| *surface == "subtle")
    {
        assert!(
            (off_ratio - 5.99).abs() < 0.005 && ratio >= off_ratio,
            "Omarchy tokyo-night placeholder {ratio:.3}:1, off {off_ratio:.3}:1"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What an application left installed: the `Palette` global, its editor
/// highlights and labels, the kit's colors and editor style as its theme
/// format writes them, and the kit fields the palette sets beside them.
fn installed(
    cx: &App,
) -> (
    Palette,
    EditorHighlights,
    Labels,
    String,
    [Option<Hsla>; 10],
) {
    let theme = Theme::global(cx);
    let kit = serde_json::to_string(&(theme.colors, &theme.highlight_theme.style)).unwrap();
    let fields = [
        theme.popover_edge,
        theme.dialog_edge,
        theme.secondary_edge,
        theme.disabled_label,
        theme.disabled_edge,
        theme.switch_hover,
        theme.ghost_hover,
        theme.ghost_active,
        theme.resting_edge,
        theme.input_fill_base,
    ];
    let labels = cx.global::<GlobalLabels>().0;
    (palette(cx), editor_highlights(cx), labels, kit, fields)
}

/// A palette's label, syntax colors by field and disabled label strength.
type Fingerprint = (&'static str, &'static [(&'static str, u32)], f32);

/// What the off path drew before More contrast existed (51a32680), in two
/// palettes whose syntax the fitting moves, a dark and a light one: the
/// fitted syntax colors by field and the disabled label's strength. The
/// hover rule of 2026-11-05 darkened Kanagawa Lotus's text and secondary
/// text, so the light guard is Lotus's tokens before then
/// ([`lotus_before_the_hover_rule`]), which still draw what 51a32680
/// drew, and the shipped Lotus is pinned as it draws since.
const OFF_PATH_FINGERPRINT: [Fingerprint; 3] = [
    (
        "Solarized Dark",
        &[
            ("keyword", 0x949acd),
            ("constant", 0xbc9a29),
            ("function", 0x33aea6),
            ("tag", 0x5ca5d4),
            ("diff.minus", 0xda8785),
        ],
        0.625,
    ),
    (
        "Kanagawa Lotus",
        &[
            ("string", 0x4a5f41),
            ("constant", 0x834c0c),
            ("type", 0x6a5638),
            ("function", 0x455982),
            ("tag", 0x4e5875),
        ],
        0.703125,
    ),
    (
        LOTUS_BEFORE_THE_HOVER_RULE,
        &[
            ("string", 0x4b5e45),
            ("constant", 0x7f4e15),
            ("type", 0x63574d),
            ("function", 0x48597e),
            ("tag", 0x535870),
        ],
        0.71875,
    ),
];

const LOTUS_BEFORE_THE_HOVER_RULE: &str = "Kanagawa Lotus before the hover rule";

/// Kanagawa Lotus's tokens before the hover rule of 2026-11-05 stepped its
/// `selected` darker and, with it, its text, secondary text and modified.
fn lotus_before_the_hover_rule() -> Palette {
    Palette {
        selected: sources::kanagawa::lotus::LOTUS_BLUE_1,
        text: sources::kanagawa::lotus::LOTUS_INK_1,
        muted: 0x5a574d,
        modified: 0x936300,
        ..ThemeChoice::KanagawaLotus.palette()
    }
}

/// With More contrast off, `Palette::apply` installs the palette as it
/// is, in every palette: the palette, the editor highlights and small
/// labels fitted from it, and the kit fields `configure` sets from it;
/// and after More contrast was on, turning it off installs exactly that
/// again. `as_applied` is the palette, and `set_more_contrast` reports
/// only a change. Those comparisons run the same helpers `apply` does,
/// so they check that the off path is the palette's own path, not that
/// it draws what it drew before More contrast existed; for that,
/// [`OFF_PATH_FINGERPRINT`] pins two palettes' fitted syntax and
/// disabled label as the off path drew them then, one of them through
/// [`lotus_before_the_hover_rule`].
#[gpui::test]
fn without_more_contrast_every_palette_applies_as_it_is(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    assert!(
        !cx.update(|cx| set_more_contrast(false, cx)),
        "off by default"
    );
    let mut pinned = 0;
    let palettes = more_contrast_palettes().into_iter().chain([(
        LOTUS_BEFORE_THE_HOVER_RULE.to_owned(),
        lotus_before_the_hover_rule(),
    )]);
    for (name, chosen) in palettes {
        let (before, drawn, dim) = cx.update(|cx| {
            chosen.apply(chosen.is_light(), None, cx);
            assert_eq!(chosen.as_applied(cx), chosen, "{name} as applied");
            let theme = Theme::global(cx);
            let dim = theme.disabled_label.map(|label| label.a);
            (installed(cx), editor_text_colors(theme), dim)
        });
        for (palette, fields, strength) in OFF_PATH_FINGERPRINT {
            if palette != name {
                continue;
            }
            pinned += 1;
            for &(field, color) in fields {
                let found = drawn.iter().find(|(drawn, _)| drawn.as_str() == field);
                assert_eq!(
                    found.and_then(|(_, color)| *color),
                    Some(color),
                    "{name} {field}"
                );
            }
            assert_eq!(dim, Some(strength), "{name} disabled label");
        }
        let expected = chosen.configured_theme(chosen.is_light());
        assert_eq!(before.0, chosen, "{name} palette");
        assert_eq!(before.1, chosen.editor_highlights(), "{name} highlights");
        assert_eq!(before.2, Labels::fit(chosen), "{name} labels");
        assert_eq!(
            before.4,
            [
                expected.popover_edge,
                expected.dialog_edge,
                expected.secondary_edge,
                expected.disabled_label,
                expected.disabled_edge,
                expected.switch_hover,
                expected.ghost_hover,
                expected.ghost_active,
                None,
                None,
            ],
            "{name} kit edges and fills"
        );
        let after = cx.update(|cx| {
            assert!(set_more_contrast(true, cx), "{name} turns it on");
            assert!(!set_more_contrast(true, cx), "{name} on again");
            chosen.apply(chosen.is_light(), None, cx);
            assert_ne!(installed(cx), before, "{name} adjusted");
            assert!(set_more_contrast(false, cx), "{name} turns it off");
            chosen.apply(chosen.is_light(), None, cx);
            installed(cx)
        });
        assert_eq!(after, before, "{name} after More contrast");
    }
    assert_eq!(pinned, OFF_PATH_FINGERPRINT.len(), "every pinned palette");
}

/// The selectors the width walk (`width_walk`) and the layout grid
/// (`layout_grid`) read: what More contrast must leave where it is.
const LAID_OUT: &[&str] = &[
    "branch-edge",
    "branch-edge-line",
    "changed-files-count",
    "changed-files-header",
    "changed-files-label",
    "changes-count-badge",
    "changes-tab",
    "code-editor",
    "commit-composer",
    "commit-description",
    "commit-inspector",
    "commit-inspector-files",
    "commit-message-toolbar",
    "commit-message-viewport",
    "commit-recovery-menu",
    "commit-staged",
    "commit-staged-count",
    "commit-title",
    "commit-title-counter",
    "compare-footer",
    "compare-toolbar",
    "copy-commit",
    "current-branch",
    "custom-themes-card",
    "edge-add-remote",
    "edge-create-branch",
    "files-pane",
    "git-actions",
    "git-actions-toggle",
    "history-clear-search",
    "history-column-0",
    "history-column-1",
    "history-column-2",
    "history-column-3",
    "history-column-4",
    "history-column-5",
    "history-empty-state",
    "history-list-edge",
    "history-no-match-line",
    "history-pane",
    "history-scope-count",
    "history-scope-name",
    "history-scope-toolbar",
    "history-search",
    "history-search-controls",
    "history-search-summary",
    "history-sidebar",
    "image-system-preview-0",
    "image-system-preview-1",
    "local-refresh-details",
    "local-refresh-warning",
    "main-menu",
    "navigation-filter",
    "navigation-hint",
    "nav-mode-local",
    "nav-mode-remote",
    "nav-mode-worktrees",
    "open-blame",
    "preview-content",
    "preview-file-path",
    "project-pane",
    "project-pane-tree",
    "repository-header",
    "repository-navigation",
    "repository-rail",
    "repository-tab-0",
    "repository-tab-strip",
    "repository-workspaces",
    "retry-local-refresh",
    "selected-changed-file",
    "settings-repository-identity",
    "settings-scroll",
    "settings-section: Appearance",
    "settings-section: Git identity",
    "settings-section: History columns",
    "settings-section: Projects",
    "stage-all",
    "staged-heading-count",
    "status-hints",
    "status-line",
    "system-preview-0",
    "system-preview-1",
    "text-mode-menu",
    "unstage-all",
    "unstaged-heading-count",
    "working-header",
    "working-header-count",
    "working-header-label",
    "working-pane",
    "working-toolbar",
];

/// The full header's controls but Changes, which [`LAID_OUT`] holds:
/// what More contrast must leave where it is in the header, so a control
/// that grows there names itself rather than the controls it moves.
const HEADER_ROW: &[&str] = &[
    "page-back",
    "repository-heading",
    "history-tab",
    "pull-requests",
    "projects",
    "profile-button",
    "settings",
];

/// The window's minimum width (`main`'s `window_min_size`), where the
/// full header's repository heading takes less than its most and more
/// than its least at 13 and 18 pt with the width walk's identity, which
/// [`crate::width_walk::open`] lands, so it gives up or takes whatever
/// any control in its row gains or loses and the controls after it move,
/// on every platform. In 1,480 px it takes its most at 13 pt, the spacer
/// taking up a control that grows there.
const SQUEEZED_HEADER_WIDTH: f32 = 1000.;

/// More contrast recolors and moves nothing. On the fixture in History,
/// then Compare, then Settings, in a 1,480 × 973 window, a 1,000 × 973
/// one ([`SQUEEZED_HEADER_WIDTH`]) and a 461 × 490 half tile, at 13 and
/// 18 pt, every selector of [`LAID_OUT`] and [`HEADER_ROW`] keeps its
/// bounds with More contrast on and off, the window drawn whole
/// (Settings is a cached view) with motion reduced and the repository's
/// identity landed and quiet reads held ([`crate::width_walk::open`]):
/// left to land later, the identity once grew the profile picker
/// between the two passes on a macOS host, More contrast blamed.
#[gpui::test]
async fn more_contrast_moves_nothing_on_any_page(cx: &mut gpui::TestAppContext) {
    use crate::width_walk;
    let (_fixture, app, cx, _) = width_walk::open(width_walk::Page::History, cx).await;
    assert_eq!(
        app.read_with(cx, |app, _| app.profile_title()),
        "Walk Fixture",
        "the walk's identity has landed before anything is measured"
    );
    cx.update(|_, cx| cx.set_reduce_motion(true));
    let mut moved = Vec::new();
    for page in ["History", "Compare", "Settings"] {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| match page {
                "Compare" => {
                    let readme = app
                        .files
                        .iter()
                        .position(|file| file.path() == std::path::Path::new("README.md"))
                        .expect("README.md among the commit's files");
                    app.select_file(readme, window, cx);
                }
                "Settings" => app.show_settings(window, cx),
                _ => {}
            })
        });
        crate::views::tests::settle(&app, cx).await;
        let mut drawn = 0;
        for (width, height) in [(1480., 973.), (SQUEEZED_HEADER_WIDTH, 973.), (461., 490.)] {
            for text in [13, 18] {
                cx.update(|window, cx| apply_text_sizes(text, DEFAULT_CODE_TEXT_SIZE, window, cx));
                cx.simulate_resize(gpui::size(px(width), px(height)));
                let [off, on] = [false, true].map(|on| {
                    cx.update(|_, cx| {
                        set_more_contrast(on, cx);
                        ThemeChoice::Midnight.apply(None, cx);
                    });
                    width_walk::frame(cx);
                    app.update(cx, |_, cx| cx.notify());
                    width_walk::frame(cx);
                    cx.update(|window, cx| {
                        window.refresh();
                        window.draw(cx).clear(cx);
                    });
                    LAID_OUT
                        .iter()
                        .chain(HEADER_ROW)
                        .map(|&selector| (selector, cx.debug_bounds(selector)))
                        .collect::<Vec<_>>()
                });
                if width == SQUEEZED_HEADER_WIDTH && page != "Settings" {
                    let heading = cx
                        .debug_bounds("repository-heading")
                        .unwrap_or_else(|| panic!("{page} at {text} pt draws the heading"));
                    let (least, most) = (ui_size(100.), ui_size(200.));
                    assert!(
                        heading.size.width > least + px(1.) && heading.size.width < most - px(1.),
                        "{page} {width}x{height} at {text} pt: the heading {heading:?} takes \
                         less than its most {most:?} and more than its least {least:?}"
                    );
                }
                drawn += off.iter().filter(|(_, bounds)| bounds.is_some()).count();
                for ((selector, off), (_, on)) in off.into_iter().zip(on) {
                    if off != on {
                        moved.push(format!(
                            "{page} {width}x{height} at {text} pt: {selector} {off:?} -> {on:?}"
                        ));
                    }
                }
            }
        }
        assert!(drawn >= 4 * 5, "{page} draws {drawn} of the selectors");
    }
    cx.update(|window, cx| {
        set_more_contrast(false, cx);
        apply_text_sizes(
            DEFAULT_INTERFACE_TEXT_SIZE,
            DEFAULT_CODE_TEXT_SIZE,
            window,
            cx,
        )
    });
    assert!(moved.is_empty(), "{}", moved.join("\n"));
}

/// The controls [`RestingControls`] draws, by kind: History's selected
/// and unselected mode toggles (`crate::button`, through `button_frame`),
/// the action bar's Fetch, a kit secondary Button (`workspace::action_frame`),
/// Settings' Save (the kit's default Button, which draws its own border),
/// a Settings switch row, unchecked, and a text field.
const RESTING_CONTROLS: [&str; 6] = [
    "selected",
    "unselected",
    "secondary",
    "default",
    "switch",
    "field",
];

/// `kind-surface`, the debug selector and ID [`RestingControls`] gives a
/// control, made once.
fn resting_selector(kind: &'static str, surface: &'static str) -> &'static str {
    thread_local! {
        static SELECTORS: std::cell::RefCell<
            std::collections::HashMap<(&'static str, &'static str), &'static str>,
        > = Default::default();
    }
    SELECTORS.with(|selectors| {
        *selectors
            .borrow_mut()
            .entry((kind, surface))
            .or_insert_with(|| disabled_selector(kind, surface))
    })
}

/// Each of [`RESTING_CONTROLS`], enabled and as the app builds it, in a
/// row on each of [`more_contrast_edge_surfaces`] of the applied palette,
/// selected as `kind-surface`, with a focus handle of its own.
struct RestingControls {
    focus: std::collections::HashMap<&'static str, gpui::FocusHandle>,
    fields: Vec<gpui::Entity<gpui_kit::component::input::InputState>>,
}

impl RestingControls {
    fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        use gpui_kit::component::input::InputState;
        use gpui_kit::{AppContext as _, Focusable as _};

        let (mut focus, mut fields) = (std::collections::HashMap::new(), Vec::new());
        for (surface, _) in more_contrast_edge_surfaces(palette(cx)) {
            for kind in RESTING_CONTROLS {
                focus.insert(resting_selector(kind, surface), cx.focus_handle());
            }
            let field = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
            focus.insert(
                resting_selector("field", surface),
                field.read(cx).focus_handle(cx),
            );
            fields.push(field);
        }
        Self { focus, fields }
    }
}

impl gpui::Render for RestingControls {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        use gpui_kit::component::button::Button;
        use gpui_kit::component::input::Input;
        use gpui_kit::{InteractiveElement as _, ParentElement as _, div};

        let surfaces = more_contrast_edge_surfaces(palette(cx));
        div()
            .flex()
            .flex_col()
            .children(surfaces.into_iter().zip(self.fields.clone()).map(
                |((surface, color), field)| {
                    let id = move |kind: &'static str| resting_selector(kind, surface);
                    let focus = |kind: &'static str| self.focus[id(kind)].clone();
                    let selector = move |kind: &'static str| move || id(kind).into();
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .bg(rgb(color))
                        .child(
                            crate::button(id("selected"), "Local", "", true)
                                .track_focus(&focus("selected"))
                                .debug_selector(selector("selected")),
                        )
                        .child(
                            crate::button(id("unselected"), "Remote", "", false)
                                .track_focus(&focus("unselected"))
                                .debug_selector(selector("unselected")),
                        )
                        .child(
                            crate::workspace::action_frame(id("secondary"))
                                .label("Fetch")
                                .track_focus(&focus("secondary"))
                                .debug_selector(selector("secondary")),
                        )
                        .child(
                            // Mirrors `settings.rs`'s Save chain: the kit's default Button.
                            Button::new(id("default"))
                                .label("Save")
                                .track_focus(&focus("default"))
                                .debug_selector(selector("default")),
                        )
                        .child(
                            div().w(px(260.)).debug_selector(selector("switch")).child(
                                crate::settings::switch_row(
                                    id("switch"),
                                    crate::settings::FOLLOW_SYSTEM,
                                    crate::settings::FOLLOW_SYSTEM,
                                    "Omarchy follows your desktop theme",
                                    &[],
                                    cx,
                                )
                                .track_focus(&focus("switch")),
                            ),
                        )
                        .child(
                            div()
                                .w(px(200.))
                                .debug_selector(selector("field"))
                                .child(Input::new(&field)),
                        )
                },
            ))
    }
}

/// The pointer and focus states a control's resting edge is read in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeState {
    Resting,
    Hovered,
    Pressed,
    Focused,
}

/// What a control painted, as [`painted_control`] reads it.
type PaintedControl = (
    gpui::Bounds<Pixels>,
    gpui::Bounds<Pixels>,
    Vec<PaintedEdge>,
    (Vec<Background>, Vec<Hsla>),
);

/// The largest filled quad the last frame painted inside `bounds` and
/// smaller than them: a switch row's track.
fn inner_fill(
    cx: &mut gpui::VisualTestContext,
    bounds: gpui::Bounds<Pixels>,
) -> Option<gpui::Bounds<Pixels>> {
    use gpui::{Bounds, point};
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let device = px(1. / scale);
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| !quad.background.is_transparent())
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
            .filter(|drawn| {
                drawn.left() >= bounds.left() - device
                    && drawn.top() >= bounds.top() - device
                    && drawn.right() <= bounds.right() + device
                    && drawn.bottom() <= bounds.bottom() + device
                    && drawn.size.width < bounds.size.width - device
            })
            .max_by(|a, b| {
                let area = |b: &Bounds<Pixels>| b.size.width.as_f32() * b.size.height.as_f32();
                area(a).total_cmp(&area(b))
            })
    })
}

/// What one frame painted for a control: its bounds, the frame its edge
/// outlines (its bounds, or a switch row's track), the bordered quads
/// inside its bounds, and its fills and the focus rings around that
/// frame (`painted_button`), with the pointer away, over it, pressing it
/// or away with the control focused.
fn painted_control(
    cx: &mut gpui::VisualTestContext,
    selector: &'static str,
    state: EdgeState,
    focus: &gpui::FocusHandle,
) -> PaintedControl {
    use gpui::{Modifiers, MouseButton, point};

    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    };
    draw(cx);
    let center = cx.debug_bounds(selector).expect(selector).center();
    let away = point(px(1.), px(1.));
    match state {
        EdgeState::Resting => cx.simulate_mouse_move(away, None, Modifiers::default()),
        EdgeState::Hovered => cx.simulate_mouse_move(center, None, Modifiers::default()),
        EdgeState::Pressed => {
            cx.simulate_mouse_move(center, None, Modifiers::default());
            cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::default());
        }
        EdgeState::Focused => {
            cx.simulate_mouse_move(away, None, Modifiers::default());
            cx.update(|window, cx| window.focus(focus, cx));
        }
    }
    draw(cx);
    let bounds = cx.debug_bounds(selector).expect(selector);
    let edges = painted_edges(cx, bounds);
    // A switch row's edge is its track's, and so is its ring.
    let frame = if selector.starts_with("switch") {
        inner_fill(cx, bounds).expect("a switch track")
    } else {
        bounds
    };
    let painted = painted_button(cx, frame);
    if state == EdgeState::Pressed {
        cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::default());
    }
    cx.simulate_mouse_move(away, None, Modifiers::default());
    cx.update(|window, cx| window.blur(cx));
    (bounds, frame, edges, painted)
}

/// The filled Buttons the kit gives no resting edge keep a boundary of
/// their own: with More contrast on, in every palette, the danger
/// Button's opaque `removed` fill (Delete theme, Discard and Force remove
/// worktree, the only tinted variant the app draws) and the primary
/// Button's opaque `accent` fill (Commit, Push and each dialog's default
/// action) read at least 3:1 on the canvas, panel, subtle and raised
/// surfaces, so each fill's own edge is its edge.
#[gpui::test]
fn more_contrast_leaves_the_danger_and_primary_fills_their_own_edge(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let mut failures = Vec::new();
    for (name, chosen) in test_palettes() {
        cx.update(|cx| {
            set_more_contrast(true, cx);
            chosen.apply(chosen.is_light(), None, cx);
            let applied = palette(cx);
            let tokens = &Theme::global(cx).tokens;
            for (variant, fill) in [
                ("danger", tokens.button_danger.background.as_solid()),
                ("primary", tokens.button_primary.background.as_solid()),
            ] {
                let Some(fill) = fill.filter(|fill| fill.a >= 1.) else {
                    failures.push(format!("{name} {variant} fill {fill:?} is not opaque"));
                    continue;
                };
                for (surface, color) in more_contrast_edge_surfaces(applied) {
                    let ratio = contrast(packed(fill), color);
                    if ratio < MORE_CONTRAST_EDGE {
                        failures.push(format!(
                            "{name} {variant} #{:06x} on {surface} {ratio:.3}:1",
                            packed(fill)
                        ));
                    }
                }
            }
        });
    }
    cx.update(|cx| set_more_contrast(false, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The palettes the resting edge is drawn in: Midnight, Braden and, on
/// Linux, Omarchy's default, Tokyo Night.
fn resting_edge_palettes() -> Vec<(String, Palette)> {
    test_palettes()
        .into_iter()
        .filter(|(name, _)| ["Midnight", "Braden", "Omarchy tokyo-night"].contains(&name.as_str()))
        .collect()
}

/// More contrast hands the kit its resting edge. With it on, in Midnight,
/// Braden and Omarchy's Tokyo Night, the selected shared compact Button
/// (History's active mode toggle), a secondary Button (Fetch), Settings'
/// Save (a default Button, with its own border), an unchecked Settings
/// switch's track and a text field each paint a 1 px edge over their
/// bounds that reads 3:1 against the surface beneath, on the canvas,
/// panel, subtle and raised surfaces at rest, and on the panel hovered,
/// pressed and focused; the unselected compact Button, transparent at
/// rest, paints none in any state. Every control keeps its bounds and its
/// focus ring with it on and off, and off, the selected, secondary and
/// switch controls paint no edge and each frame is the one drawn before
/// More contrast was turned on. The switch's track is a fill, not an
/// edge: in every state it paints the fill it paints off, and the thumb
/// reads on it as it does off; filled with the adjusted `border`, it hid
/// the edge and the thumb fell from 10.3:1 to 4.8:1 on it (Midnight).
#[gpui::test]
fn more_contrast_edges_the_controls_that_rest_on_a_fill(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(RestingControls::new);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(gpui::size(px(1400.), px(600.)));
    let palettes = resting_edge_palettes();
    assert_eq!(
        palettes.len(),
        if cfg!(target_os = "linux") { 3 } else { 2 }
    );
    let mut failures = Vec::new();
    for (name, chosen) in palettes {
        let mut passes: Vec<Vec<(&str, &str, EdgeState, PaintedControl)>> = Vec::new();
        // The unchecked Switch's thumb in each pass, the kit's `switch_thumb`.
        let mut thumbs = Vec::new();
        for on in [false, true, false] {
            cx.update(|window, cx| {
                set_more_contrast(on, cx);
                chosen.apply(chosen.is_light(), Some(window), cx);
            });
            thumbs.push(cx.update(|_, cx| packed(Theme::global(cx).switch_thumb)));
            let mut drawn = Vec::new();
            for (surface, _) in more_contrast_edge_surfaces(chosen) {
                let states: &[EdgeState] = if surface == "panel" {
                    &[
                        EdgeState::Resting,
                        EdgeState::Hovered,
                        EdgeState::Pressed,
                        EdgeState::Focused,
                    ]
                } else {
                    &[EdgeState::Resting]
                };
                for kind in RESTING_CONTROLS {
                    let selector = disabled_selector(kind, surface);
                    let focus = view.read_with(cx, |view, _| view.focus[selector].clone());
                    for &state in states {
                        let painted = painted_control(cx, selector, state, &focus);
                        drawn.push((kind, surface, state, painted));
                    }
                }
            }
            passes.push(drawn);
        }
        let edge = chosen.with_more_contrast().resting_edge();
        for ((before, on), after) in passes[0].iter().zip(&passes[1]).zip(&passes[2]) {
            let (kind, surface, state, (bounds, frame, edges, painted)) = on;
            let at = format!("{name} {kind} on {surface} {state:?}");
            if before.3 != after.3 {
                failures.push(format!(
                    "{at}: off after on {:?}, before {:?}",
                    after.3, before.3
                ));
            }
            let (off_bounds, off_frame, off_edges, off_painted) = &before.3;
            if (bounds, frame) != (off_bounds, off_frame) {
                failures.push(format!(
                    "{at}: bounds {bounds:?} {frame:?}, off {off_bounds:?} {off_frame:?}"
                ));
            }
            if painted.1 != off_painted.1 {
                failures.push(format!(
                    "{at}: rings {:?}, off {:?}",
                    painted.1, off_painted.1
                ));
            }
            // A field shows its focus in its own border, read below.
            if *state == EdgeState::Focused && *kind != "field" && painted.1.is_empty() {
                failures.push(format!("{at}: draws no focus ring"));
            }
            // A track is a fill, not an edge: it rests and lifts as it
            // does off, and its thumb reads on it as it does off.
            if *kind == "switch" {
                let track = |fills: &[Background]| {
                    fills.last().and_then(|fill| fill.as_solid()).map(packed)
                };
                let (thumb, off_thumb) = (thumbs[1], thumbs[0]);
                match (track(&painted.0), track(&off_painted.0)) {
                    (Some(track), Some(off_track)) => {
                        let (ratio, off_ratio) =
                            (contrast(thumb, track), contrast(off_thumb, off_track));
                        if painted.0 != off_painted.0 || ratio < off_ratio {
                            failures.push(format!(
                                "{at}: track #{track:06x}, its thumb {ratio:.3}:1; off \
                                 #{off_track:06x}, {off_ratio:.3}:1"
                            ));
                        }
                    }
                    fills => failures.push(format!("{at}: track fills {fills:?}")),
                }
            }
            let kit_edged = matches!(*kind, "selected" | "secondary" | "switch");
            if *kind == "unselected" {
                if !edges.is_empty() {
                    failures.push(format!("{at}: paints {edges:?}"));
                }
                continue;
            }
            if kit_edged && !off_edges.is_empty() {
                failures.push(format!("{at}: off paints {off_edges:?}"));
            }
            let reading = edges.iter().find(|drawn| {
                drawn.one_px
                    && drawn
                        .beneath
                        .is_some_and(|beneath| contrast(drawn.color, beneath) >= MORE_CONTRAST_EDGE)
            });
            let Some(reading) = reading else {
                failures.push(format!("{at}: no 1 px edge reading 3:1 in {edges:?}"));
                continue;
            };
            // The kit's own layer is the palette's resting edge, over
            // the whole control or the switch's track.
            let over = |a: gpui::Bounds<Pixels>, b: gpui::Bounds<Pixels>| {
                [
                    (a.left(), b.left()),
                    (a.top(), b.top()),
                    (a.right(), b.right()),
                    (a.bottom(), b.bottom()),
                ]
                .into_iter()
                .all(|(a, b)| (a - b).abs() < px(0.5))
            };
            if kit_edged && (reading.color != edge || !over(reading.bounds, *frame)) {
                failures.push(format!(
                    "{at}: edge {reading:?}, not #{edge:06x} over {frame:?}"
                ));
            }
        }
    }
    cx.update(|window, cx| {
        set_more_contrast(false, cx);
        ThemeChoice::Midnight.apply(Some(window), cx);
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Under More contrast the editor highlights keep the rules decided on
/// 2026-11-05, in every palette and in [`faint_custom_palette`], a
/// palette made from a built-in; a word steps across its line on each
/// side outside the Omarchy palettes, so a run without them (macOS)
/// reads the step on both sides. Each changed-word tint stands beyond
/// its line tint from the canvas: further from it in OKLab lightness, on
/// the same side, and at least [`HIGHLIGHT_DISTANCE`] from the line, with
/// every color drawn on it as text (`text`, every syntax role but
/// `removed` and the patch text the Diff view draws on it) reading the
/// text rule; a word that stepped across, one that does not stand beyond
/// its line as lifted ([`stands_beyond`]: drawn off on its canvas side, or
/// left nearer it than [`HIGHLIGHT_DISTANCE`] by its line's move), stands
/// [`HIGHLIGHT_LIGHTNESS_GAP`] beyond its line in lightness, or short of it only where no step that
/// far reads the text rule ([`the_gap_cannot_read`]: One Dark's added
/// word, whose raised keyword would read 4.39:1).
/// Find's other matches stand [`HIGHLIGHT_DISTANCE`] and
/// [`HIGHLIGHT_LIGHTNESS_GAP`] in lightness from the selection, the
/// current match and both word tints, [`HIGHLIGHT_DISTANCE`] from every
/// editor line, and every color drawn on them as text (`text` and every
/// role, the patch text among them) reads the text rule; and
/// the selection and the current match stand as far apart, with the
/// gap. With More contrast off, 31 word tints across the palettes lay on
/// their line's canvas side (Braden's removed word `#fdf7f8` on its line
/// `#f9e5eb`), Find's other matches drew the selection's colour in every
/// palette, and the selection and current match stood 0.003 apart in
/// Sandstone and 0.044 apart in One Dark by chroma alone; held to its
/// distance from its line alone, Solarized Dark's added word stepped
/// across to 0.0004 beyond it.
#[gpui::test]
fn more_contrast_keeps_the_highlight_rules_in_every_palette(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let distance = |a: u32, b: u32| oklab_distance(oklab(a), oklab(b));
    let lightness = |a: u32, b: u32| (oklab(a)[0] - oklab(b)[0]).abs();
    let mut failures = Vec::new();
    let mut crossed_in = [Vec::new(), Vec::new()];
    for (name, chosen) in more_contrast_palettes() {
        let (applied, on) = cx.update(|cx| {
            set_more_contrast(true, cx);
            chosen.apply(chosen.is_light(), None, cx);
            (palette(cx), editor_highlights(cx))
        });
        let roles = applied.syntax_roles(Some(chosen));
        // The words as lifted, or drawn off on the canvas side, before
        // any steps across.
        let lifted = words_lines_move_for(chosen);
        for (index, side, (diff, line), word, drawn) in [
            (
                0,
                "added",
                (chosen.added, applied.added_background),
                on.added_word,
                [on.patch_text.added],
            ),
            (
                1,
                "removed",
                (chosen.removed, applied.removed_background),
                on.removed_word,
                [on.patch_text.removed],
            ),
        ] {
            // Short of standing beyond its line as the line was moved
            // for it, drawn off on its canvas side or left nearer it than
            // [`HIGHLIGHT_DISTANCE`] by its line's move, the word stepped
            // across.
            let crossed = !stands_beyond(lifted[index], line, chosen);
            let beyond = lightness_beyond(word, line, chosen);
            let colors: Vec<u32> = roles.word_colors().into_iter().chain(drawn).collect();
            let short = crossed
                && beyond < HIGHLIGHT_LIGHTNESS_GAP
                && !the_gap_cannot_read(&colors, diff, line, chosen);
            if beyond <= 0. || short {
                failures.push(format!(
                    "{name} {side} word #{word:06x} {beyond:+.4} beyond its line \
                     #{line:06x}{}",
                    if crossed { ", stepped across" } else { "" }
                ));
            }
            if distance(word, line) < HIGHLIGHT_DISTANCE {
                failures.push(format!(
                    "{name} {side} word #{word:06x} {:.4} from its line #{line:06x}",
                    distance(word, line)
                ));
            }
            for color in colors {
                if contrast(color, word) < LABEL_RULE {
                    failures.push(format!(
                        "{name} #{color:06x} on the {side} word #{word:06x} {:.3}:1",
                        contrast(color, word)
                    ));
                }
            }
            if crossed {
                crossed_in[index].push(name.clone());
            }
        }
        for (other, color) in [
            ("selection", on.selection),
            ("current match", on.find_current),
            ("added word", on.added_word),
            ("removed word", on.removed_word),
        ] {
            if distance(on.find_match, color) < HIGHLIGHT_DISTANCE
                || lightness(on.find_match, color) < HIGHLIGHT_LIGHTNESS_GAP
            {
                failures.push(format!(
                    "{name} Find match #{:06x} {:.4} from the {other} #{color:06x}, {:.4} in \
                     lightness",
                    on.find_match,
                    distance(on.find_match, color),
                    lightness(on.find_match, color)
                ));
            }
        }
        let lines = EditorHighlights::lines(applied);
        for line in lines {
            if distance(on.find_match, line) < HIGHLIGHT_DISTANCE {
                failures.push(format!(
                    "{name} Find match #{:06x} {:.4} from the line #{line:06x}",
                    on.find_match,
                    distance(on.find_match, line)
                ));
            }
        }
        for color in roles.colors() {
            if contrast(color, on.find_match) < LABEL_RULE {
                failures.push(format!(
                    "{name} #{color:06x} on the Find match #{:06x} {:.3}:1",
                    on.find_match,
                    contrast(color, on.find_match)
                ));
            }
        }
        if distance(on.selection, on.find_current) < HIGHLIGHT_DISTANCE
            || lightness(on.selection, on.find_current) < HIGHLIGHT_LIGHTNESS_GAP
        {
            failures.push(format!(
                "{name} current match #{:06x} {:.4} from the selection #{:06x}, {:.4} in \
                 lightness",
                on.find_current,
                distance(on.selection, on.find_current),
                on.selection,
                lightness(on.selection, on.find_current)
            ));
        }
    }
    cx.update(|cx| set_more_contrast(false, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Words drawn off on their line's canvas side step across on both
    // sides, in the built-in palettes and the palette made from one.
    for (side, names) in ["added", "removed"].into_iter().zip(&crossed_in) {
        assert!(
            names.iter().any(|name| !name.starts_with("Omarchy ")),
            "no {side} word drawn off on its line's canvas side outside the Omarchy \
             palettes: {names:?}"
        );
    }
}

/// With More contrast off, the selection, Find's other and current
/// matches and the changed-word tints draw exactly the colours they drew
/// before More contrast's highlight rules (a dark and a light palette),
/// and no changed word draws a rule: Off is the palette's tuned look and
/// runs none of those rules.
#[gpui::test]
fn more_contrast_off_draws_the_highlights_as_before(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    for (choice, drawn) in [
        (
            ThemeChoice::Midnight,
            [0x0b3e3e, 0x0b3e3e, 0x004836, 0x114939, 0x5f3241],
        ),
        (
            ThemeChoice::Daylight,
            [0xbff6e7, 0xbff6e7, 0x93fde6, 0xc3f5de, 0xfcf6f7],
        ),
    ] {
        let highlights = cx.update(|cx| {
            set_more_contrast(false, cx);
            choice.apply(None, cx);
            editor_highlights(cx)
        });
        assert_eq!(
            [
                highlights.selection,
                highlights.find_match,
                highlights.find_current,
                highlights.added_word,
                highlights.removed_word,
            ],
            drawn,
            "{choice:?}: selection, Find match, current match, added and removed words"
        );
        assert_eq!(
            (highlights.added_rule, highlights.removed_rule),
            (None, None),
            "{choice:?}: no changed-word rule"
        );
    }
}

/// A hovered row never reads heavier than the selected row, in every
/// built-in and Omarchy palette and in [`faint_custom_palette`], with
/// More contrast off and on: on the canvas, the panel and `subtle`,
/// `hover` stands nearer than `selected` in OKLab lightness, in ΔEOK and
/// in WCAG luminance contrast, and both move from each surface the same
/// way, away from the canvas (darker in a light palette, lighter in a
/// dark one), in OKLab lightness and in luminance, so hover over
/// `subtle` never turns toward the canvas. One Light's hover stood 0.020
/// further from its canvas in lightness than its selected row, Kanagawa
/// Wave's 0.011 and Kanagawa Lotus's 0.0015, until One Light's
/// `selected` (`#e9edff` to `#e0e4f6`), Kanagawa Wave's `hover`
/// (`#363646` to `#323242`) and Kanagawa Lotus's `selected` (`#c7d7e0` to
/// `#c4d4dd`) were tuned; Lotus's hover lightened to `#ede6bc` instead
/// stood lighter than its `subtle` (`#e5ddb0`), so a hovered row on it
/// turned toward the canvas, 1.051:1 from the canvas where the selected
/// row stands 1.230:1.
#[gpui::test]
fn a_hovered_row_never_reads_heavier_than_the_selected_row(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let mut failures = Vec::new();
    for (name, chosen) in more_contrast_palettes() {
        for more in [false, true] {
            let applied = cx.update(|cx| {
                set_more_contrast(more, cx);
                chosen.apply(chosen.is_light(), None, cx);
                palette(cx)
            });
            let (hover, selected) = (applied.hover, applied.selected);
            for (surface, background) in [
                ("canvas", applied.canvas),
                ("panel", applied.panel),
                ("subtle", applied.subtle),
            ] {
                let lightness = |color: u32| (oklab(color)[0] - oklab(background)[0]).abs();
                let apart = |color: u32| oklab_distance(oklab(color), oklab(background));
                let ratio = |color: u32| contrast(color, background);
                if lightness(hover) >= lightness(selected)
                    || apart(hover) >= apart(selected)
                    || ratio(hover) >= ratio(selected)
                {
                    failures.push(format!(
                        "{name}, More contrast {more}: hover #{hover:06x} {:.4} from the \
                         {surface} #{background:06x} in lightness, {:.4} in ΔEOK and \
                         {:.4}:1, the selected row #{selected:06x} {:.4}, {:.4} and {:.4}:1",
                        lightness(hover),
                        apart(hover),
                        ratio(hover),
                        lightness(selected),
                        apart(selected),
                        ratio(selected)
                    ));
                }
                // Away from the canvas: darker in a light palette, lighter in a dark one.
                let side = if applied.is_light() {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
                let away = |color: u32| {
                    oklab(color)[0].total_cmp(&oklab(background)[0]) == side
                        && luminance(color).total_cmp(&luminance(background)) == side
                };
                for (state, color) in [("hover", hover), ("selected row", selected)] {
                    if !away(color) {
                        failures.push(format!(
                            "{name}, More contrast {more}: the {state} #{color:06x} moves \
                             from the {surface} #{background:06x} toward the canvas \
                             #{:06x}",
                            applied.canvas
                        ));
                    }
                }
            }
        }
    }
    cx.update(|cx| set_more_contrast(false, cx));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
