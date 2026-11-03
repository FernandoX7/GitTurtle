//! An element laid out for the room it has in the frame it is drawn.
//!
//! Some layouts choose what they show from the height they are given: a label
//! that gives way before content does, or a line that shows the whole lines
//! its room holds. Chosen from the height a frame before laid out, such a
//! choice is a frame late: the first frame after a resize, or after an answer
//! arrives, draws the old choice and asks for another. [`InFrame`] builds
//! what it holds as it is prepainted, for its bounds in that frame, as
//! `uniform_list` builds its rows and the Working inspector's body does
//! (`workspace::WorkingBody`). Nothing it holds asks for another frame to
//! catch up.
//!
//! What it holds is built only once its bounds are known, so it cannot size
//! its parent: an [`InFrame`] reports no size of its own and takes only the
//! room its parent leaves it, a share of the parent's free height as a
//! `flex_1` child with a zero minimum. Its parent must give it a set height:
//! a fixed height, `size_full` or a flex share of a parent that has one. A
//! parent whose height comes from its content gives it none, where a `flex_1`
//! div would have grown to what it holds, and so does a parent whose other
//! children take all of its height, as an image comparison's toolbars can in
//! a very short window; from inside, the two look alike, so nothing checks
//! for the first. A narrow document side's wrapped header controls once took
//! all of their side's height too; the side's header is now laid out in the
//! side's own frame and bounded there (`system_preview::header_ends`), so it
//! leaves the frame beneath it at least the rule's pixel and, where a click
//! answered, the line's one whole line.

use crate::*;

/// What an [`InFrame`] builds, for its bounds in the frame being drawn.
type Build = Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> AnyElement>;

/// The element [`in_frame`] returns.
pub(crate) struct InFrame {
    build: Option<Build>,
    /// The name its frames are recorded under in tests ([`probe`]).
    #[cfg(test)]
    name: Option<&'static str>,
}

/// An element that takes the room its parent leaves it, at least none, and
/// holds what `build` makes for its bounds in each frame, laid out there.
/// Its parent must give it a set height, as the module's notes say.
pub(crate) fn in_frame(
    build: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> AnyElement + 'static,
) -> InFrame {
    InFrame {
        build: Some(Box::new(build)),
        #[cfg(test)]
        name: None,
    }
}

impl InFrame {
    /// Names the region, so a test can read where each frame laid it and
    /// its [`probed`] parts out ([`probe`]); outside tests it does nothing.
    pub(crate) fn named(self, name: &'static str) -> Self {
        #[cfg(test)]
        {
            Self {
                name: Some(name),
                ..self
            }
        }
        #[cfg(not(test))]
        {
            let _ = name;
            self
        }
    }
}

impl IntoElement for InFrame {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for InFrame {
    type RequestLayoutState = ();
    /// What it holds, laid out in its bounds.
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // `flex_1`, as small as nothing, and no size of its own: what it holds
        // is built only once its bounds are known.
        let style = Style {
            flex_grow: 1.,
            flex_shrink: 1.,
            flex_basis: relative(0.).into(),
            min_size: Size {
                width: px(0.).into(),
                height: px(0.).into(),
            },
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let build = self.build.take()?;
        #[cfg(test)]
        if let Some(name) = self.name {
            probe::open(name, bounds);
        }
        let mut element = build(bounds, window, cx);
        // Laid out on its bounds' whole device pixels, as a set width is
        // ([`on_whole_device_pixels`]).
        let scale = window.scale_factor();
        element.layout_as_root(
            size(
                AvailableSpace::Definite(on_whole_device_pixels(bounds.size.width, scale)),
                AvailableSpace::Definite(on_whole_device_pixels(bounds.size.height, scale)),
            ),
            window,
            cx,
        );
        element.prepaint_at(bounds.origin, window, cx);
        #[cfg(test)]
        if self.name.is_some() {
            probe::close();
        }
        Some(element)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        element: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(element) = element {
            element.paint(window, cx);
        }
    }
}

/// `length`, a side of bounds GPUI laid out on whole device pixels on a
/// display of `scale`, as layout reads it back: never short of those
/// pixels. GPUI multiplies a definite room by the scale without rounding
/// (`TaffyLayoutEngine::compute_layout`), while a set length such as
/// `.w(…)` is rounded to a whole device pixel first (`to_taffy`), so bounds
/// 243 device pixels wide at scale 1.2, 202.49998 px, reached layout as
/// 242.99998 device pixels, and what a prediction measured at a set width
/// of 243 (as [`measured_height`] does) to fill its line wrapped: held to
/// the narrower side's line (`pdf_view::pair_line`), which kept a device
/// pixel spare for it, a PDF comparison's Before header took three lines
/// beside After's two at 18 pt in 203.75 px sides without that pixel. The
/// smallest length whose product with the scale is the whole device
/// pixels, or the next float above them where none is exact.
fn on_whole_device_pixels(length: Pixels, scale: f32) -> Pixels {
    let device = (f32::from(length) * scale).round();
    let mut snapped = device / scale;
    while snapped * scale < device {
        snapped = snapped.next_up();
    }
    while snapped > 0. && snapped.next_down() * scale >= device {
        snapped = snapped.next_down();
    }
    px(snapped)
}

/// The height `element` takes laid out `width` wide, for a choice made while
/// an [`InFrame`] builds; the element is laid out and dropped, never drawn.
pub(crate) fn measured_height(
    element: impl IntoElement,
    width: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Pixels {
    div()
        .w(width)
        .child(element)
        .into_any_element()
        .layout_as_root(
            size(AvailableSpace::Definite(width), AvailableSpace::MinContent),
            window,
            cx,
        )
        .height
}

/// `element`, whose bounds a test reads as `name` in the frame its named
/// region was laid out in ([`probe`]); outside tests, `element` as it is.
pub(crate) fn probed<E: ParentElement>(element: E, name: &'static str) -> E {
    #[cfg(test)]
    {
        use gpui_kit::base::ElementExt as _;
        element.on_prepaint(move |bounds, _, _| probe::part(name, bounds))
    }
    #[cfg(not(test))]
    {
        let _ = name;
        element
    }
}

/// Test-only: a record of every frame that lays out a named [`InFrame`],
/// with the bounds of its [`probed`] parts. A test window draws a frame that
/// a layout asks for at once, inside the update that asked, so bounds read
/// after that update show only the last of them; this record keeps each.
#[cfg(test)]
pub(crate) mod probe {
    use super::*;
    use std::cell::RefCell;

    /// One frame's layout of a region.
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct Frame {
        pub(crate) region: &'static str,
        pub(crate) bounds: Bounds<Pixels>,
        /// Each probed part drawn in it, in the order it was laid out.
        pub(crate) parts: Vec<(&'static str, Bounds<Pixels>)>,
    }

    impl Frame {
        /// The bounds of the part `name`, where it was drawn.
        pub(crate) fn part(&self, name: &str) -> Option<Bounds<Pixels>> {
            self.parts
                .iter()
                .find(|(part, _)| *part == name)
                .map(|(_, bounds)| *bounds)
        }
    }

    thread_local! {
        /// The frames recorded since [`watch`], while watching.
        static FRAMES: RefCell<Option<Vec<Frame>>> = const { RefCell::new(None) };
        /// The named regions being laid out, innermost last, each with the
        /// index of its frame in [`FRAMES`] where one is recorded.
        static OPEN: RefCell<Vec<Option<usize>>> = const { RefCell::new(Vec::new()) };
    }

    /// Starts recording, forgetting anything recorded before.
    pub(crate) fn watch() {
        FRAMES.with_borrow_mut(|frames| *frames = Some(Vec::new()));
    }

    /// The frames recorded since [`watch`] or the last call, which keeps
    /// recording.
    pub(crate) fn take() -> Vec<Frame> {
        FRAMES.with_borrow_mut(|frames| frames.as_mut().map(std::mem::take).unwrap_or_default())
    }

    /// Stops recording.
    pub(crate) fn stop() {
        FRAMES.with_borrow_mut(|frames| *frames = None);
    }

    /// Starts the frame of `region`, laid out in `bounds`, until [`close`].
    pub(super) fn open(region: &'static str, bounds: Bounds<Pixels>) {
        let index = FRAMES.with_borrow_mut(|frames| {
            frames.as_mut().map(|frames| {
                frames.push(Frame {
                    region,
                    bounds,
                    parts: Vec::new(),
                });
                frames.len() - 1
            })
        });
        OPEN.with_borrow_mut(|open| open.push(index));
    }

    /// Ends the innermost region's frame.
    pub(super) fn close() {
        OPEN.with_borrow_mut(|open| open.pop());
    }

    /// Records a part in the frame of the innermost region being laid out;
    /// a part drawn outside every named region belongs to none of them.
    pub(super) fn part(name: &'static str, bounds: Bounds<Pixels>) {
        let Some(Some(index)) = OPEN.with_borrow(|open| open.last().copied()) else {
            return;
        };
        FRAMES.with_borrow_mut(|frames| {
            if let Some(frame) = frames.as_mut().and_then(|frames| frames.get_mut(index)) {
                frame.parts.push((name, bounds));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    /// A frame `device` device pixels wide holding a wrapping row of two
    /// boxes that fill it exactly, each a set width on whole device pixels.
    struct Filled {
        device: f32,
    }

    impl Render for Filled {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let scale = window.scale_factor();
            let first = (self.device / 2.).floor();
            let second = self.device - first;
            div()
                .w(px(self.device / scale))
                .h(px(40.))
                .flex()
                .flex_col()
                .child(in_frame(move |_, _, _| {
                    div()
                        .flex()
                        .flex_wrap()
                        .child(
                            div()
                                .debug_selector(|| "filled-first".into())
                                .w(px(first / scale))
                                .h(px(10.)),
                        )
                        .child(
                            div()
                                .debug_selector(|| "filled-second".into())
                                .w(px(second / scale))
                                .h(px(10.)),
                        )
                        .into_any_element()
                }))
        }
    }

    /// What an [`InFrame`] holds is laid out on its frame's whole device
    /// pixels, as a set width is: at scales 1.1, 1.2, 1.25, 1.5, 1.75 and 2,
    /// in frames 100 to 400 device pixels wide, a row of two boxes that fill
    /// the frame exactly lies on one line. Laid out at the frame's width as
    /// GPUI reads it back, 243 device pixels at scale 1.2 reached layout as
    /// 242.99998, and the second box wrapped beneath the first.
    #[gpui::test]
    fn what_it_holds_is_laid_out_on_whole_device_pixels(cx: &mut gpui_kit::TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, _| Filled { device: 243. });
        let mut wrapped = Vec::new();
        for scale in [1.1f32, 1.2, 1.25, 1.5, 1.75, 2.] {
            cx.update(|window, _| window.set_scale_factor(scale));
            for device in 100..=400 {
                view.update(cx, |view, cx| {
                    view.device = device as f32;
                    cx.notify();
                });
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
                let first = cx.debug_bounds("filled-first").expect("the first box");
                let second = cx.debug_bounds("filled-second").expect("the second box");
                if second.top() != first.top() {
                    wrapped.push((scale, device));
                }
            }
        }
        assert!(
            wrapped.is_empty(),
            "the second box wrapped at (scale, device pixels) {wrapped:?}"
        );
    }
}
