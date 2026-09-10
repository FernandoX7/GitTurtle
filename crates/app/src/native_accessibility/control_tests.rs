use crate::*;
use core::prelude::v1::test;
use gpui::prelude::FluentBuilder as _;
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
