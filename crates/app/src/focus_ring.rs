//! A list's keyboard focus ring, kept while the pointer moves.
//!
//! GPUI's `focus_visible` styles a focused element only while the last input
//! was a key, so merely moving the pointer took the ring off a list that kept
//! keyboard focus. Here a key press shows the ring of whatever holds focus,
//! and the ring stays whatever the pointer does until a pointer press, as GTK
//! and browsers' `:focus-visible` behave: a click still focuses a list without
//! marking it, and the next key marks it.
use crate::*;
use std::{cell::Cell, rc::Rc};

/// One window's keyboard focus ring, owned by its [`GitTurtle`].
#[derive(Default)]
pub(crate) struct FocusRing {
    /// A key was pressed after the last pointer press. Shared with the
    /// window's pointer-press listener, [`FocusRing::pointer_listener`].
    keyboard: Rc<Cell<bool>>,
    /// The focus whose ring the frame being built draws.
    ringed: Option<FocusHandle>,
}

impl FocusRing {
    /// Reads, before a frame builds its lists, which focus draws its ring.
    pub(crate) fn begin_frame(&mut self, window: &Window, cx: &App) {
        // GPUI marks every key press as keyboard input and redraws the
        // window when input turns from the pointer to keys, so a frame
        // follows each press that shows the ring.
        if window.last_input_was_keyboard() {
            self.keyboard.set(true);
        }
        self.ringed = self.keyboard.get().then(|| window.focused(cx)).flatten();
    }

    /// Whether `focus` draws its keyboard ring this frame.
    pub(crate) fn shows(&self, focus: &FocusHandle) -> bool {
        self.ringed.as_ref() == Some(focus)
    }

    /// An empty element whose paint ends the ring at any pointer press in the
    /// window, over a dialog's backdrop included: GPUI calls a window's mouse
    /// listeners for every press, whatever lies under the pointer.
    pub(crate) fn pointer_listener(&self) -> impl IntoElement {
        let keyboard = self.keyboard.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let keyboard = keyboard.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, _| {
                    if phase == DispatchPhase::Capture && keyboard.replace(false) {
                        window.refresh();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }
}

/// An outline the last frame painted, read back for tests: its bounds and
/// width in logical pixels, and whether it is whole.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintedOutline {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) width: Pixels,
    pub(crate) whole: bool,
}

/// The outlines of `color`, borders with no fill, that the last frame
/// painted. GPUI paints such a border as one strip per side, each masked to
/// its side and to every clip around it, and drops a strip a clip hides
/// whole; the clips are rectangles, so an outline is whole when its four
/// strips are drawn and their masks together reach its bounds.
#[cfg(test)]
pub(crate) fn painted_outlines(
    cx: &mut gpui_kit::VisualTestContext,
    color: Hsla,
) -> Vec<PaintedOutline> {
    use gpui_kit::ScaledPixels;
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let logical = |value: ScaledPixels| px(value.as_f32() / scale);
        let strips: Vec<_> = window
            .painted_quads()
            .into_iter()
            .filter(|quad| quad.background.is_transparent() && quad.border_color == color)
            .map(|quad| {
                (
                    quad.bounds,
                    quad.border_widths.top,
                    quad.content_mask.bounds,
                )
            })
            .collect();
        let mut outlines: Vec<PaintedOutline> = Vec::new();
        let mut seen = Vec::new();
        for (bounds, width, _) in &strips {
            if seen.contains(bounds) {
                continue;
            }
            seen.push(*bounds);
            let masks: Vec<_> = strips
                .iter()
                .filter(|(other, _, _)| other == bounds)
                .map(|(_, _, mask)| *mask)
                .collect();
            let covered = masks.iter().copied().reduce(|a, b| a.union(&b)).unwrap();
            let whole = masks.len() == 4
                && covered.left() <= bounds.left()
                && covered.top() <= bounds.top()
                && covered.right() >= bounds.right()
                && covered.bottom() >= bounds.bottom();
            outlines.push(PaintedOutline {
                bounds: Bounds::from_corners(
                    point(logical(bounds.left()), logical(bounds.top())),
                    point(logical(bounds.right()), logical(bounds.bottom())),
                ),
                width: logical(*width),
                whole,
            });
        }
        outlines
    })
}

/// The focused kit Input whose text lies in `text` draws its focus whole in
/// the last frame: its own 1 px edge in the theme's ring colour, the accent,
/// and the kit's ring outside that edge, every side of both inside the clips
/// around the field.
#[cfg(test)]
pub(crate) fn assert_input_ringed(
    cx: &mut gpui_kit::VisualTestContext,
    text: Bounds<Pixels>,
    field: &str,
) {
    use gpui_kit::component::{FocusRing, Theme};
    let kit = FocusRing::default();
    let (edge_color, ring_color) = cx.read(|cx| {
        let ring = Theme::global(cx).ring;
        (ring, ring.alpha(kit.opacity))
    });
    let edges: Vec<_> = painted_outlines(cx, edge_color)
        .into_iter()
        .filter(|edge| edge.bounds.contains(&text.center()))
        .collect();
    let [edge] = edges[..] else {
        panic!("{field}: one accent edge around the field, {text:?}: {edges:?}");
    };
    assert_eq!(edge.width, px(1.), "{field}: {edge:?}");
    assert!(
        edge.whole,
        "{field}: a clip cuts the field's edge: {edge:?}"
    );
    let around = edge.bounds.dilate(kit.gap + kit.width);
    let slack = px(1.);
    let rings: Vec<_> = painted_outlines(cx, ring_color)
        .into_iter()
        .filter(|ring| {
            [
                (ring.bounds.left(), around.left()),
                (ring.bounds.top(), around.top()),
                (ring.bounds.right(), around.right()),
                (ring.bounds.bottom(), around.bottom()),
            ]
            .into_iter()
            .all(|(drawn, expected)| (drawn - expected).abs() <= slack)
        })
        .collect();
    let [ring] = rings[..] else {
        panic!("{field}: one ring around the field's edge, {around:?}: {rings:?}");
    };
    assert!(
        ring.whole,
        "{field}: a clip cuts the field's ring: {ring:?}"
    );
}
