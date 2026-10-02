use crate::*;
use core::prelude::v1::test;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::component::{FocusRing, FocusableExt as _, Theme, ThemeStyled as _};
use std::{cell::RefCell, rc::Rc};

#[gpui::test]
fn button_content_preserves_explicit_type_and_icon_geometry(cx: &mut TestAppContext) {
    type Observations = Rc<RefCell<Vec<(Pixels, Pixels)>>>;
    struct Probe(Observations);
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(200.)).flex().flex_col().children(
                [(None, None), (Some(18.), Some(24.)), (Some(12.), Some(16.))]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (font, icon_size))| {
                        let observations = self.0.clone();
                        Button::new(("control-geometry", index))
                            .small()
                            .w(px(200.))
                            .h(px(40.))
                            .px_0()
                            .gap_0()
                            .when_some(font, |button, size| button.text_size(px(size)))
                            .icon(
                                Icon::default()
                                    .when_some(icon_size, |icon, size| icon.size(px(size))),
                            )
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        let font = window
                                            .text_style()
                                            .font_size
                                            .to_pixels(window.rem_size());
                                        // The inner horizontal content is centered in 200 points.
                                        // A zero-width probe immediately follows the icon with
                                        // zero gap, so its distance from center is half the icon.
                                        observations.borrow_mut()[index] =
                                            (font, (bounds.origin.x - px(100.)) * 2.);
                                    },
                                    |_, _, _, _| {},
                                )
                                .w(px(0.))
                                .h(px(1.)),
                            )
                    }),
            )
        }
    }
    cx.update(gpui_kit::init);
    let observations: Observations = Rc::new(RefCell::new(vec![(px(0.), px(0.)); 3]));
    let observed = observations.clone();
    let (_, cx) = cx.add_window_view(move |window, _| {
        window.set_rem_size(px(13.));
        Probe(observations)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for ((font, icon), (expected_font, expected_icon)) in
        observed
            .borrow()
            .iter()
            .zip([(11.375, 11.375), (18., 24.), (12., 16.)])
    {
        assert!(
            (f32::from(*font) - expected_font).abs() < 0.01,
            "font {font:?}, expected {expected_font}"
        );
        // GPUI snaps the default fractional rem geometry to device pixels.
        assert!(
            (f32::from(*icon) - expected_icon).abs() <= 0.5,
            "icon {icon:?}, expected {expected_icon}"
        );
    }
}

/// The Buttons the focus ring regression focuses, with their labels:
/// borderless, primary, and disabled with the default variant's 1 px border.
const RING_BUTTONS: [(&str, &str); 3] = [
    ("ring-ghost", "ring-ghost-label"),
    ("ring-primary", "ring-primary-label"),
    ("ring-disabled", "ring-disabled-label"),
];

struct RingProbe {
    focus: [FocusHandle; 3],
    ring_enabled: bool,
}

impl Render for RingProbe {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_8()
            .flex()
            .gap_8()
            .children(
                RING_BUTTONS
                    .into_iter()
                    .zip(&self.focus)
                    .map(|((id, label), focus)| {
                        Button::new(id)
                            .map(|button| match id {
                                "ring-ghost" => button.ghost(),
                                "ring-primary" => button.primary(),
                                _ => button.disabled(true),
                            })
                            .track_focus(focus)
                            .focus_ring(self.ring_enabled)
                            .debug_selector(|| id.into())
                            .child(div().debug_selector(|| label.into()).child("Save"))
                    }),
            )
            // The ring every other control draws, through `focus_ring_style`.
            .child(
                div()
                    .debug_selector(|| "ring-kit".into())
                    .size_8()
                    .border_1()
                    .rounded_md()
                    .focus_ring_style(window, cx),
            )
    }
}

/// A quad in the rendered scene, in logical pixels.
#[derive(Debug, PartialEq)]
struct PaintedQuad {
    outer: Bounds<Pixels>,
    /// The outer bounds less the border: the inner edge of a ring.
    inner: Bounds<Pixels>,
    fill: Background,
    border: Hsla,
    radii: [Pixels; 4],
}

/// The rendered scene's quads. GPUI paints a border-only quad as four copies,
/// each clipped to one side; they count once.
fn painted_quads(cx: &mut VisualTestContext) -> Vec<PaintedQuad> {
    let quads = cx.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |value: ScaledPixels| px(value.as_f32() / scale);
        window
            .painted_quads()
            .into_iter()
            .map(|quad| {
                let (origin, extent, widths) =
                    (quad.bounds.origin, quad.bounds.size, quad.border_widths);
                let outer = Bounds::new(
                    point(logical(origin.x), logical(origin.y)),
                    size(logical(extent.width), logical(extent.height)),
                );
                let inner = Bounds::from_corners(
                    point(
                        outer.left() + logical(widths.left),
                        outer.top() + logical(widths.top),
                    ),
                    point(
                        outer.right() - logical(widths.right),
                        outer.bottom() - logical(widths.bottom),
                    ),
                );
                let radii = quad.corner_radii;
                PaintedQuad {
                    outer,
                    inner,
                    fill: quad.background,
                    border: quad.border_color,
                    radii: [
                        radii.top_left,
                        radii.top_right,
                        radii.bottom_right,
                        radii.bottom_left,
                    ]
                    .map(logical),
                }
            })
            .collect::<Vec<_>>()
    });
    let mut distinct = Vec::new();
    for quad in quads {
        if !distinct.contains(&quad) {
            distinct.push(quad);
        }
    }
    distinct
}

fn near(drawn: Bounds<Pixels>, expected: Bounds<Pixels>, device: Pixels) -> bool {
    [
        (drawn.left(), expected.left()),
        (drawn.top(), expected.top()),
        (drawn.right(), expected.right()),
        (drawn.bottom(), expected.bottom()),
    ]
    .into_iter()
    .all(|(edge, expected)| (edge - expected).abs() <= device)
}

/// The bordered quads drawn wholly outside `element`: its focus ring.
fn rings_around(
    quads: &[PaintedQuad],
    element: Bounds<Pixels>,
    device: Pixels,
) -> Vec<&PaintedQuad> {
    quads
        .iter()
        .filter(|quad| {
            quad.inner != quad.outer
                && quad.outer.left() < element.left() - device
                && quad.outer.top() < element.top() - device
                && quad.outer.right() > element.right() + device
                && quad.outer.bottom() > element.bottom() + device
        })
        .collect()
}

/// The corner radii of the quads `element` paints on its own bounds.
fn own_radii(quads: &[PaintedQuad], element: Bounds<Pixels>, device: Pixels) -> Vec<[Pixels; 4]> {
    quads
        .iter()
        .filter(|quad| near(quad.outer, element, device))
        .map(|quad| quad.radii)
        .collect()
}

/// `element`, rounded by `radius`, draws exactly one ring, `width` wide and
/// `gap` outside its edge, with corners concentric with its own, in `ring` at
/// `alpha`.
fn assert_ring(
    name: &str,
    quads: &[PaintedQuad],
    (element, radius): (Bounds<Pixels>, Pixels),
    (width, gap, alpha): (f32, f32, f32),
    ring: Hsla,
    device: Pixels,
) {
    let rings = rings_around(quads, element, device);
    let [drawn] = rings.as_slice() else {
        panic!("{name} draws one ring around {element:?}, drew {rings:?}");
    };
    let outer = element.dilate(px(gap + width));
    assert!(
        near(drawn.outer, outer, device),
        "{name}: ring's outer edge {:?}, expected {outer:?}",
        drawn.outer
    );
    let inner = element.dilate(px(gap));
    assert!(
        near(drawn.inner, inner, device),
        "{name}: ring's inner edge {:?}, expected {inner:?}",
        drawn.inner
    );
    // Radii are not snapped to device pixels, so they match exactly.
    let corners = radius + px(gap + width);
    assert!(
        drawn
            .radii
            .iter()
            .all(|drawn| (*drawn - corners).abs() < px(0.01)),
        "{name}: ring's corner radii {:?}, expected {corners:?}",
        drawn.radii
    );
    let expected = ring.alpha(alpha);
    let channels = |color: Hsla| [color.h, color.s, color.l, color.a];
    assert!(
        channels(drawn.border)
            .into_iter()
            .zip(channels(expected))
            .all(|(drawn, expected)| (drawn - expected).abs() < 1e-4),
        "{name}: ring drawn in {:?}, expected {expected:?}",
        drawn.border
    );
}

/// A focused Button, borderless, primary or disabled, draws the ring
/// `Theme::button_focus_ring` describes through the child outside its edge,
/// which moves nothing. At its default that is the ring every other control
/// draws, and the setting leaves theirs alone. `Theme::focus_ring` and
/// `Button::focus_ring(false)` still turn the ring off.
#[gpui::test]
fn focused_buttons_draw_the_theme_button_focus_ring(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (probe, cx) = cx.add_window_view(|_, cx| RingProbe {
        focus: [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()],
        ring_enabled: true,
    });
    let draw = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    };
    let device = cx.update(|window, _| px(1. / window.scale_factor()));
    // Every probe Button is rounded by the theme radius.
    let (ring, radius) = cx.read(|cx| (Theme::global(cx).ring, Theme::global(cx).radius));
    let kit_ring = (3., 0., 0.5);
    let adopted = FocusRing {
        width: px(2.),
        gap: px(1.),
        opacity: 1.,
    };

    for (setting, expected) in [(FocusRing::default(), kit_ring), (adopted, (2., 1., 1.))] {
        cx.update(|window, cx| {
            Theme::global_mut(cx).button_focus_ring = setting;
            window.blur(cx);
        });
        draw(cx);
        let quads = painted_quads(cx);
        let resting = RING_BUTTONS.map(|(id, label)| {
            let bounds = cx.debug_bounds(id).expect("rendered Button");
            let label = cx.debug_bounds(label).expect("rendered label");
            assert!(
                rings_around(&quads, bounds, device).is_empty(),
                "unfocused {id} draws no ring under {setting:?}"
            );
            let radii = own_radii(&quads, bounds, device);
            // A resting ghost Button paints no quad of its own, so its radius
            // shows only in the ring's concentric corners; the primary and the
            // bordered disabled Button paint theirs.
            assert_eq!(
                radii.is_empty(),
                id == "ring-ghost",
                "{id} paints {radii:?}"
            );
            assert!(
                radii.iter().all(|corners| *corners == [radius; 4]),
                "{id} rounded by {radii:?}"
            );
            (bounds, label, radii)
        });
        let kit = cx.debug_bounds("ring-kit").expect("rendered kit ring");
        let kit = (kit, own_radii(&quads, kit, device)[0][0]);
        assert_ring("focus_ring_style", &quads, kit, kit_ring, ring, device);

        for (index, (id, label_id)) in RING_BUTTONS.into_iter().enumerate() {
            let handle = cx.read(|cx| probe.read(cx).focus[index].clone());
            cx.update(|window, cx| window.focus(&handle, cx));
            draw(cx);
            assert!(cx.update(|window, _| handle.is_focused(window)), "{id}");
            let quads = painted_quads(cx);
            let (bounds, label, radii) = &resting[index];
            let name = format!("focused {id} under {setting:?}");
            assert_eq!(cx.debug_bounds(id).as_ref(), Some(bounds), "{name}");
            assert_eq!(
                cx.debug_bounds(label_id).as_ref(),
                Some(label),
                "{name}: label"
            );
            assert_eq!(&own_radii(&quads, *bounds, device), radii, "{name}: radius");
            assert_ring(&name, &quads, (*bounds, radius), expected, ring, device);
            assert_ring("focus_ring_style", &quads, kit, kit_ring, ring, device);
        }
    }

    // The switches that turn the ring off still win over the setting.
    let handle = cx.read(|cx| probe.read(cx).focus[0].clone());
    cx.update(|window, cx| {
        window.focus(&handle, cx);
        Theme::global_mut(cx).focus_ring = false;
    });
    draw(cx);
    let quads = painted_quads(cx);
    let ghost = cx.debug_bounds(RING_BUTTONS[0].0).expect("rendered Button");
    assert!(
        rings_around(&quads, ghost, device).is_empty(),
        "Theme::focus_ring off"
    );
    cx.update(|_, cx| {
        Theme::global_mut(cx).focus_ring = true;
        probe.update(cx, |probe, cx| {
            probe.ring_enabled = false;
            cx.notify();
        });
    });
    draw(cx);
    let quads = painted_quads(cx);
    assert!(
        rings_around(&quads, ghost, device).is_empty(),
        "Button::focus_ring(false)"
    );
}

/// A caller's hover styles a Button only while it is enabled: a disabled
/// Button keeps its disabled fill under the pointer and while pressed, and an
/// enabled one still takes the hover.
#[gpui::test]
fn caller_hover_styles_only_enabled_buttons(cx: &mut TestAppContext) {
    const BUTTONS: [(&str, bool); 2] = [("hover-enabled", false), ("hover-disabled", true)];
    fn mark() -> Hsla {
        hsla(0.83, 1., 0.5, 1.)
    }
    struct Probe;
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .p_8()
                .flex()
                .gap_8()
                .children(BUTTONS.map(|(id, disabled)| {
                    // Selected, like the shared helper's active control, so the kit
                    // registers no hover of its own.
                    Button::new(id)
                        .selected(true)
                        .disabled(disabled)
                        .label("Open")
                        .hover(|style| style.bg(mark()))
                        .debug_selector(|| id.into())
                }))
        }
    }
    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|_, _| Probe);
    let draw = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    };
    let device = cx.update(|window, _| px(1. / window.scale_factor()));
    let fills = |cx: &mut VisualTestContext, bounds: Bounds<Pixels>| {
        painted_quads(cx)
            .into_iter()
            .filter(|quad| near(quad.outer, bounds, device))
            .map(|quad| quad.fill)
            .collect::<Vec<_>>()
    };
    let mark = Background::from(mark());
    draw(cx);
    for (id, disabled) in BUTTONS {
        let bounds = cx.debug_bounds(id).expect("rendered Button");
        let resting = fills(cx, bounds);
        assert!(!resting.contains(&mark), "{id} at rest paints {resting:?}");

        cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
        draw(cx);
        let hovered = fills(cx, bounds);
        if disabled {
            assert_eq!(hovered, resting, "{id} hovered");
        } else {
            assert!(hovered.contains(&mark), "{id} hovered paints {hovered:?}");
        }

        cx.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
        draw(cx);
        if disabled {
            assert_eq!(fills(cx, bounds), resting, "{id} pressed");
        }
        cx.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
        draw(cx);
        assert_eq!(fills(cx, bounds), resting, "{id} after the pointer leaves");
    }
}

/// A kit tooltip wider than the window wraps inside it, with the tooltip
/// positioner's margin and the popup's own on both sides; one that fits keeps
/// its unwrapped layout. Each tooltip is laid out as the tooltip overlay lays
/// out a shown one, above its trigger, in a fill of its own.
#[gpui::test]
fn tooltips_wider_than_the_window_wrap_inside_it(cx: &mut TestAppContext) {
    use crate::text_review::OPTIONS_TOOLTIP;
    use gpui_kit::{base::TooltipPositioner, component::kbd::Kbd};

    /// gpui-base's tooltip `WINDOW_MARGIN`, kept from the viewport's edges.
    const POSITIONER_MARGIN: f32 = 4.;
    const WIDTH: f32 = 461.;
    const SHORT: &str = "Previous change";
    /// Text, trigger left and top, and whether it carries a key binding: the
    /// Options tooltip clamped at either edge and with a binding, and a short
    /// tooltip. Each trigger leaves room above for the wrapped popup, which
    /// the positioner would otherwise place below it.
    const TOOLTIPS: [(&str, f32, f32, bool); 4] = [
        (OPTIONS_TOOLTIP, 40., 220., false),
        (OPTIONS_TOOLTIP, 420., 300., false),
        (SHORT, 216., 380., false),
        (OPTIONS_TOOLTIP, 216., 460., true),
    ];
    fn fill(index: usize) -> Hsla {
        hsla(index as f32 / 8., 1., 0.5, 1.)
    }
    struct Probe(Vec<(Bounds<Pixels>, AnyView)>);
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children(self.0.iter().map(|(trigger, tooltip)| {
                    deferred(TooltipPositioner::new(*trigger).child(div().child(tooltip.clone())))
                }))
        }
    }

    cx.update(gpui_kit::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        Probe(
            TOOLTIPS
                .iter()
                .enumerate()
                .map(|(index, &(text, left, top, binding))| {
                    let tooltip = Tooltip::new(text)
                        .bg(fill(index))
                        .when(binding, |tooltip| {
                            tooltip.key_binding(Some(Kbd::new(
                                Keystroke::parse("ctrl-shift-k").unwrap(),
                            )))
                        })
                        .build(window, cx);
                    let trigger = Bounds::new(point(px(left), px(top)), size(px(28.), px(28.)));
                    (trigger, tooltip)
                })
                .collect(),
        )
    });
    let device = cx.update(|window, _| px(1. / window.scale_factor()));
    let popups = |cx: &mut VisualTestContext, width: f32, rem: f32| {
        cx.simulate_resize(size(px(width), px(490.)));
        cx.update(|window, cx| {
            window.set_rem_size(px(rem));
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        let quads = painted_quads(cx);
        (0..TOOLTIPS.len())
            .map(|index| {
                let fill = Background::from(fill(index));
                let popup = quads
                    .iter()
                    .filter(|quad| quad.fill == fill)
                    .map(|quad| quad.outer)
                    .collect::<Vec<_>>();
                assert_eq!(popup.len(), 1, "tooltip {index} at {width}: {popup:?}");
                popup[0]
            })
            .collect::<Vec<_>>()
    };

    // At the default interface size and an enlarged one, whose rem the
    // popup's margin follows.
    for rem in [16., 18.] {
        let wide = popups(cx, 2000., rem);
        let narrow = popups(cx, WIDTH, rem);
        let edge = px(POSITIONER_MARGIN + 0.75 * rem);
        let one_line = wide[2].size.height;
        // Less the vertical padding (`py_0p5`) and border.
        let line = one_line - px(2. * (0.125 * rem + 1.));
        for (index, (&(text, _, top, _), (wide, narrow))) in
            TOOLTIPS.iter().zip(wide.iter().zip(&narrow)).enumerate()
        {
            let at = format!("tooltip {index} at rem {rem}: {narrow:?}, unwrapped {wide:?}");
            assert_eq!(wide.size.height, one_line, "one line unwrapped, {at}");
            assert!(
                narrow.bottom() <= px(top) + device,
                "above its trigger, {at}"
            );
            if text == SHORT {
                assert_eq!(narrow.size, wide.size, "fits and is unchanged, {at}");
                continue;
            }
            assert!(wide.size.width > px(WIDTH), "wider than the window, {at}");
            assert!(
                (narrow.left() - edge).abs() <= device
                    && (px(WIDTH) - narrow.right() - edge).abs() <= device,
                "inset by {edge:?} on both sides, {at}"
            );
            assert!(narrow.size.height >= one_line + line, "wraps, {at}");
        }
    }
}

/// The bare kit controls that track their focus handle only while enabled.
const DISABLING_CONTROLS: [&str; 6] = [
    "radio",
    "toggle",
    "link",
    "color-swatch",
    "checkbox",
    "switch",
];

/// One bare kit control between two tab stops the probe owns.
struct DisablingProbe {
    before: FocusHandle,
    after: FocusHandle,
    control: &'static str,
    disabled: bool,
}

impl Render for DisablingProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::base;
        let disabled = self.disabled;
        let control = match self.control {
            "radio" => base::Radio::new("probe-control")
                .disabled(disabled)
                .size(px(20.))
                .into_any_element(),
            "toggle" => base::Toggle::new("probe-control")
                .disabled(disabled)
                .size(px(20.))
                .into_any_element(),
            "link" => base::Link::new("probe-control")
                .disabled(disabled)
                .size(px(20.))
                .into_any_element(),
            "color-swatch" => base::ColorSwatch::new("probe-control", gpui::red())
                .disabled(disabled)
                .size(px(20.))
                .into_any_element(),
            "checkbox" => base::Checkbox::new("probe-control")
                .disabled(disabled)
                .size(px(20.))
                .into_any_element(),
            "switch" => base::Switch::new("probe-control")
                .disabled(disabled)
                .size(px(20.))
                .into_any_element(),
            other => unreachable!("{other}"),
        };
        div()
            .flex()
            .gap_2()
            .child(
                div()
                    .id("probe-before")
                    .track_focus(&self.before.clone().tab_stop(true))
                    .size(px(20.)),
            )
            .child(control)
            .child(
                div()
                    .id("probe-after")
                    .track_focus(&self.after.clone().tab_stop(true))
                    .size(px(20.)),
            )
    }
}

/// Each bare kit Radio, Toggle, Link, ColorPicker swatch, Checkbox and Switch
/// that turns disabled while it holds focus still lets Tab and Shift+Tab,
/// sent as keystrokes, leave it for its neighbours; disabled and unfocused,
/// Tab skips it, and enabled again it is a tab stop again.
#[gpui::test]
fn tab_and_shift_tab_leave_focused_kit_controls_that_turn_disabled(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    for control in DISABLING_CONTROLS {
        let captured: Rc<RefCell<Option<Entity<DisablingProbe>>>> = Default::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let probe = cx.new(|cx| DisablingProbe {
                before: cx.focus_handle(),
                after: cx.focus_handle(),
                control,
                disabled: false,
            });
            *output.borrow_mut() = Some(probe.clone());
            gpui_kit::component::Root::new(probe, window, cx)
        });
        let probe = captured.borrow().as_ref().unwrap().clone();
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        };
        let press = |cx: &mut VisualTestContext, keys: &str| {
            cx.simulate_keystrokes(keys);
            draw(cx);
            cx.update(|window, cx| window.focused(cx))
                .expect("a focused element")
        };
        let disable = |cx: &mut VisualTestContext, disabled: bool| {
            cx.update(|_, cx| {
                probe.update(cx, |probe, cx| {
                    probe.disabled = disabled;
                    cx.notify();
                })
            });
            draw(cx);
        };
        let (before, after) = cx.read(|cx| {
            let probe = probe.read(cx);
            (probe.before.clone(), probe.after.clone())
        });
        let focus = |cx: &mut VisualTestContext, handle: &FocusHandle| {
            cx.update(|window, cx| window.focus(handle, cx));
            draw(cx);
        };

        draw(cx);
        focus(cx, &before);
        let handle = press(cx, "tab");
        assert!(
            handle != before && handle != after,
            "Tab reaches the {control}"
        );
        assert_eq!(press(cx, "tab"), after, "{control} precedes the after stop");
        assert_eq!(press(cx, "shift-tab"), handle);

        disable(cx, true);
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            Some(handle.clone()),
            "the disabled {control} keeps focus until the keyboard moves it"
        );
        assert_eq!(press(cx, "tab"), after, "Tab leaves the disabled {control}");
        assert_eq!(
            press(cx, "shift-tab"),
            before,
            "Tab and Shift+Tab skip the disabled, unfocused {control}"
        );
        assert_eq!(press(cx, "tab"), after);

        disable(cx, false);
        focus(cx, &before);
        assert_eq!(
            press(cx, "tab"),
            handle,
            "enabled again, the {control} is a tab stop again"
        );
        disable(cx, true);
        assert_eq!(
            press(cx, "shift-tab"),
            before,
            "Shift+Tab leaves the disabled {control}"
        );
    }
}

/// One bare kit control, or a component Button, between two tab stops the
/// probe owns, with the click or change handler that records each run and
/// updates the state the probe passes back to it.
struct ActivationProbe {
    before: FocusHandle,
    after: FocusHandle,
    control: &'static str,
    disabled: bool,
    /// The control's checked, pressed or selected state; a Link has none, so
    /// its handler only marks the activation.
    on: bool,
    /// Click and change handler runs.
    runs: usize,
}

impl ActivationProbe {
    fn record(probe: &WeakEntity<Self>, on: bool, cx: &mut App) {
        probe
            .update(cx, |probe, cx| {
                probe.on = on;
                probe.runs += 1;
                cx.notify();
            })
            .expect("the probe is alive");
    }
}

impl Render for ActivationProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::base;
        let (disabled, on) = (self.disabled, self.on);
        let probe = cx.entity().downgrade();
        let control = match self.control {
            "radio" => base::Radio::new("probe-control")
                .checked(on)
                .disabled(disabled)
                .on_change(move |checked, _, _, cx| Self::record(&probe, checked, cx))
                .size(px(20.))
                .into_any_element(),
            "toggle" => base::Toggle::new("probe-control")
                .pressed(on)
                .disabled(disabled)
                .on_change(move |pressed, _, _, cx| Self::record(&probe, pressed, cx))
                .size(px(20.))
                .into_any_element(),
            "link" => base::Link::new("probe-control")
                .disabled(disabled)
                .on_activate(move |_, _, cx| Self::record(&probe, true, cx))
                .size(px(20.))
                .into_any_element(),
            "color-swatch" => base::ColorSwatch::new("probe-control", gpui::red())
                .selected(on)
                .disabled(disabled)
                .on_click(move |_, _, _, cx| Self::record(&probe, true, cx))
                .size(px(20.))
                .into_any_element(),
            "checkbox" => base::Checkbox::new("probe-control")
                .checked(on)
                .disabled(disabled)
                .on_change(move |state, _, _, cx| {
                    Self::record(&probe, state == base::CheckboxState::Checked, cx)
                })
                .size(px(20.))
                .into_any_element(),
            "switch" => base::Switch::new("probe-control")
                .checked(on)
                .disabled(disabled)
                .on_change(move |checked, _, _, cx| Self::record(&probe, checked, cx))
                .size(px(20.))
                .into_any_element(),
            "button" => Button::new("probe-control")
                .label("Open")
                .selected(on)
                .disabled(disabled)
                .on_click(move |_, _, cx| Self::record(&probe, !on, cx))
                .into_any_element(),
            other => unreachable!("{other}"),
        };
        div()
            .flex()
            .gap_2()
            .child(
                div()
                    .id("probe-before")
                    .track_focus(&self.before.clone().tab_stop(true))
                    .size(px(20.)),
            )
            .child(control)
            .child(
                div()
                    .id("probe-after")
                    .track_focus(&self.after.clone().tab_stop(true))
                    .size(px(20.)),
            )
    }
}

/// Enter and Space, sent as key-down and key-up events, activate each bare kit
/// Radio, Toggle, Link, ColorPicker swatch, Checkbox and Switch, and a
/// component Button, while it is enabled; once it turns disabled while it holds
/// focus, neither key runs its click or change handler or changes its state,
/// and focus stays on it until Tab moves it on.
#[gpui::test]
fn enter_and_space_leave_focused_kit_controls_that_turn_disabled_inert(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    for control in DISABLING_CONTROLS.into_iter().chain(["button"]) {
        let captured: Rc<RefCell<Option<Entity<ActivationProbe>>>> = Default::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let probe = cx.new(|cx| ActivationProbe {
                before: cx.focus_handle(),
                after: cx.focus_handle(),
                control,
                disabled: false,
                on: false,
                runs: 0,
            });
            *output.borrow_mut() = Some(probe.clone());
            gpui_kit::component::Root::new(probe, window, cx)
        });
        let probe = captured.borrow().as_ref().unwrap().clone();
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        };
        let focused = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.focused(cx))
                .expect("a focused element")
        };
        // A keyboard click runs on the key-up that follows a key-down on the
        // same focused element, so each key sends both, as a real press does.
        let activate = |cx: &mut VisualTestContext, key: &str| {
            let keystroke = Keystroke::parse(key).unwrap();
            cx.simulate_event(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            cx.simulate_event(KeyUpEvent { keystroke });
            draw(cx);
        };
        let set = |cx: &mut VisualTestContext, disabled: bool| {
            cx.update(|_, cx| {
                probe.update(cx, |probe, cx| {
                    probe.disabled = disabled;
                    probe.on = false;
                    probe.runs = 0;
                    cx.notify();
                })
            });
            draw(cx);
        };
        let state = |cx: &mut VisualTestContext| {
            cx.read(|cx| {
                let probe = probe.read(cx);
                (probe.on, probe.runs)
            })
        };
        let (before, after) = cx.read(|cx| {
            let probe = probe.read(cx);
            (probe.before.clone(), probe.after.clone())
        });

        draw(cx);
        cx.update(|window, cx| window.focus(&before, cx));
        draw(cx);
        cx.simulate_keystrokes("tab");
        draw(cx);
        let handle = focused(cx);
        assert!(
            handle != before && handle != after,
            "Tab reaches the {control}"
        );

        // Enabled, each key reaches the focused control and activates it.
        for key in ["enter", "space"] {
            set(cx, false);
            activate(cx, key);
            assert_eq!(
                state(cx),
                (true, 1),
                "{key} activates the enabled {control}"
            );
            assert_eq!(focused(cx), handle, "{key} keeps focus on the {control}");
        }

        set(cx, true);
        assert_eq!(
            focused(cx),
            handle,
            "the disabled {control} keeps focus until the keyboard moves it"
        );
        for key in ["enter", "space", "enter"] {
            activate(cx, key);
            assert_eq!(
                state(cx),
                (false, 0),
                "{key} runs no handler and changes no state on the disabled {control}"
            );
            assert_eq!(
                focused(cx),
                handle,
                "{key} keeps focus on the disabled {control}"
            );
        }
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(focused(cx), after, "Tab leaves the disabled {control}");
    }
}
