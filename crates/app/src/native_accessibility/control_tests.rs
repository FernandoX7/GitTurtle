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

/// `element` draws exactly one ring, `width` wide and `gap` outside its edge,
/// in `ring` at `alpha`.
fn assert_ring(
    name: &str,
    quads: &[PaintedQuad],
    element: Bounds<Pixels>,
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
    let ring = cx.read(|cx| Theme::global(cx).ring);
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
            (bounds, label, own_radii(&quads, bounds, device))
        });
        let kit = cx.debug_bounds("ring-kit").expect("rendered kit ring");
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
            assert_ring(&name, &quads, *bounds, expected, ring, device);
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
